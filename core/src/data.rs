use std::{
    collections::HashMap,
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
    process::{self, Stdio},
    str::FromStr,
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use crossbeam_utils::sync::WaitGroup;
use druid::{
    theme, Application, Color, Command, Data, Env, EventCtx, ExtEventSink,
    FontDescriptor, FontFamily, KeyEvent, Lens, Point, Rect, Size, Target, Vec2,
    WidgetId, WindowId,
};
use im;
use lsp_types::{
    CompletionItem, CompletionResponse, CompletionTextEdit, GotoDefinitionResponse,
    Location, Position,
};
use parking_lot::Mutex;
use serde_json::Value;
use tree_sitter_highlight::{Highlight, HighlightEvent, Highlighter};
use xi_core_lib::selection::InsertDrift;
use xi_rope::{
    spans::SpansBuilder, DeltaBuilder, Interval, Rope, RopeDelta, Transformer,
};
use xi_rpc::{RpcLoop, RpcPeer};

use crate::{
    buffer::{
        has_unmatched_pair, previous_has_unmatched_pair, Buffer, BufferId,
        BufferNew, BufferState, BufferUpdate, EditType, Style, UpdateEvent,
    },
    command::{
        EnsureVisiblePosition, LapceCommand, LapceUICommand, LAPCE_UI_COMMAND,
    },
    completion::{CompletionData, CompletionStatus, Snippet},
    editor::EditorLocationNew,
    keypress::{KeyPressData, KeyPressFocus},
    language::new_highlight_config,
    movement::{Cursor, CursorMode, LinePosition, Movement, SelRegion, Selection},
    palette::{PaletteData, PaletteType, PaletteViewData},
    proxy::{LapceProxy, ProxyHandlerNew},
    split::SplitMoveDirection,
    state::{LapceWorkspace, LapceWorkspaceType, Mode, VisualMode},
    theme::LapceTheme,
};

#[derive(Clone, Data)]
pub struct LapceData {
    pub windows: im::HashMap<WindowId, LapceWindowData>,
    pub theme: Arc<std::collections::HashMap<String, Color>>,
    pub keypress: Arc<KeyPressData>,
}

impl LapceData {
    pub fn load() -> Self {
        let mut windows = im::HashMap::new();
        let keypress = Arc::new(KeyPressData::new());
        let theme =
            Arc::new(Self::get_theme().unwrap_or(std::collections::HashMap::new()));
        let window = LapceWindowData::new(keypress.clone(), theme.clone());
        windows.insert(WindowId::next(), window);
        Self {
            windows,
            theme,
            keypress,
        }
    }

    fn get_theme() -> Result<std::collections::HashMap<String, Color>> {
        let mut f = File::open("/Users/Lulu/lapce/.lapce/theme.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        let toml_theme: im::HashMap<String, String> = toml::from_slice(&content)?;

        let mut theme = std::collections::HashMap::new();
        for (name, hex) in toml_theme.iter() {
            if let Ok(color) = Color::from_hex_str(hex) {
                theme.insert(name.to_string(), color);
            }
        }
        Ok(theme)
    }

    pub fn reload_env(&self, env: &mut Env) {
        let changed = match env.try_get(&LapceTheme::CHANGED) {
            Ok(changed) => changed,
            Err(e) => true,
        };
        if !changed {
            return;
        }
        env.set(theme::SCROLLBAR_RADIUS, 0.0);
        env.set(theme::SCROLLBAR_EDGE_WIDTH, 0.0);
        env.set(theme::SCROLLBAR_WIDTH, 10.0);
        env.set(theme::SCROLLBAR_PAD, 0.0);
        env.set(
            theme::SCROLLBAR_COLOR,
            Color::from_hex_str("#949494").unwrap(),
        );

        env.set(LapceTheme::CHANGED, false);
        let theme = &self.theme;
        if let Some(line_highlight) = theme.get("line_highlight") {
            env.set(
                LapceTheme::EDITOR_CURRENT_LINE_BACKGROUND,
                line_highlight.clone(),
            );
        };
        if let Some(caret) = theme.get("caret") {
            env.set(LapceTheme::EDITOR_CURSOR_COLOR, caret.clone());
        };
        if let Some(foreground) = theme.get("foreground") {
            env.set(LapceTheme::EDITOR_FOREGROUND, foreground.clone());
        };
        if let Some(background) = theme.get("background") {
            env.set(LapceTheme::EDITOR_BACKGROUND, background.clone());
        };
        if let Some(selection) = theme.get("selection") {
            env.set(LapceTheme::EDITOR_SELECTION_COLOR, selection.clone());
        };
        if let Some(color) = theme.get("comment") {
            env.set(LapceTheme::EDITOR_COMMENT, color.clone());
        };
        if let Some(color) = theme.get("error") {
            env.set(LapceTheme::EDITOR_ERROR, color.clone());
        };
        if let Some(color) = theme.get("warn") {
            env.set(LapceTheme::EDITOR_WARN, color.clone());
        };
        env.set(LapceTheme::EDITOR_LINE_HEIGHT, 25.0);
        env.set(LapceTheme::PALETTE_BACKGROUND, Color::rgb8(125, 125, 125));
        env.set(LapceTheme::PALETTE_INPUT_FOREROUND, Color::rgb8(0, 0, 0));
        env.set(
            LapceTheme::PALETTE_INPUT_BACKGROUND,
            Color::rgb8(255, 255, 255),
        );
        env.set(LapceTheme::PALETTE_INPUT_BORDER, Color::rgb8(0, 0, 0));
        env.set(
            LapceTheme::EDITOR_FONT,
            FontDescriptor::new(FontFamily::new_unchecked("Cascadia Code"))
                .with_size(13.0),
        );
    }
}

#[derive(Clone)]
pub struct LapceWindowData {
    pub tabs: im::HashMap<WidgetId, LapceTabData>,
    pub active: WidgetId,
    pub keypress: Arc<KeyPressData>,
    pub theme: Arc<std::collections::HashMap<String, Color>>,
}

impl Data for LapceWindowData {
    fn same(&self, other: &Self) -> bool {
        self.active == other.active && self.tabs.same(&other.tabs)
    }
}

impl LapceWindowData {
    pub fn new(
        keypress: Arc<KeyPressData>,
        theme: Arc<std::collections::HashMap<String, Color>>,
    ) -> Self {
        let mut tabs = im::HashMap::new();
        let tab_id = WidgetId::next();
        let tab = LapceTabData::new(tab_id, keypress.clone(), theme.clone());
        tabs.insert(tab_id, tab);
        Self {
            tabs,
            active: tab_id,
            keypress,
            theme,
        }
    }
}

#[derive(Clone, Lens)]
pub struct LapceTabData {
    pub id: WidgetId,
    pub workspace: Arc<LapceWorkspace>,
    pub main_split: LapceMainSplitData,
    pub completion: Arc<CompletionData>,
    pub palette: Arc<PaletteData>,
    pub proxy: Arc<LapceProxy>,
    pub keypress: Arc<KeyPressData>,
    pub update_receiver: Option<Receiver<UpdateEvent>>,
    pub update_sender: Arc<Sender<UpdateEvent>>,
    pub theme: Arc<std::collections::HashMap<String, Color>>,
    pub window_origin: Point,
}

impl Data for LapceTabData {
    fn same(&self, other: &Self) -> bool {
        self.main_split.same(&other.main_split)
            && self.completion.same(&other.completion)
            && self.palette.same(&other.palette)
    }
}

impl LapceTabData {
    pub fn new(
        tab_id: WidgetId,
        keypress: Arc<KeyPressData>,
        theme: Arc<std::collections::HashMap<String, Color>>,
    ) -> Self {
        let (update_sender, update_receiver) = unbounded();
        let update_sender = Arc::new(update_sender);
        let proxy = Arc::new(LapceProxy::new(tab_id));
        let palette = Arc::new(PaletteData::new(proxy.clone()));
        let main_split = LapceMainSplitData::new(
            palette.preview_editor,
            update_sender.clone(),
            proxy.clone(),
        );
        let completion = Arc::new(CompletionData::new());
        Self {
            id: tab_id,
            workspace: Arc::new(LapceWorkspace {
                kind: LapceWorkspaceType::Local,
                path: PathBuf::from("/Users/Lulu/lapce"),
            }),
            main_split,
            completion,
            palette,
            proxy,
            keypress,
            theme,
            update_sender,
            update_receiver: Some(update_receiver),
            window_origin: Point::ZERO,
        }
    }

    pub fn completion_origin(&self, tab_size: Size, env: &Env) -> Point {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);

        let editor = self.main_split.active_editor();
        let buffer = self.main_split.open_files.get(&editor.buffer).unwrap();
        let offset = self.completion.offset;
        let (line, col) = buffer.offset_to_line_col(offset);
        let width = 7.6171875;
        let x = col as f64 * width - line_height - 5.0;
        let y = (line + 1) as f64 * line_height;
        let mut origin =
            editor.window_origin - self.window_origin.to_vec2() + Vec2::new(x, y);
        if origin.y + self.completion.size.height + 1.0 > tab_size.height {
            let height = self
                .completion
                .size
                .height
                .min(self.completion.len() as f64 * line_height);
            origin.y = editor.window_origin.y - self.window_origin.y
                + line as f64 * line_height
                - height;
        }
        if origin.x + self.completion.size.width + 1.0 > tab_size.width {
            origin.x = tab_size.width - self.completion.size.width - 1.0;
        }
        if origin.x <= 0.0 {
            origin.x = 0.0;
        }

        origin
    }

    pub fn palette_view_data(&self) -> PaletteViewData {
        PaletteViewData {
            palette: self.palette.clone(),
            workspace: self.workspace.clone(),
            main_split: self.main_split.clone(),
            keypress: self.keypress.clone(),
        }
    }

    pub fn buffer_update_process(
        tab_id: WidgetId,
        receiver: Receiver<UpdateEvent>,
        event_sink: ExtEventSink,
    ) {
        use std::collections::{HashMap, HashSet};
        fn insert_update(
            updates: &mut HashMap<BufferId, UpdateEvent>,
            event: UpdateEvent,
        ) {
            let update = match &event {
                UpdateEvent::Buffer(update) => update,
                UpdateEvent::SemanticTokens(update, tokens) => update,
            };
            if let Some(current) = updates.get(&update.id) {
                let current = match &event {
                    UpdateEvent::Buffer(update) => update,
                    UpdateEvent::SemanticTokens(update, tokens) => update,
                };
                if update.rev > current.rev {
                    updates.insert(update.id, event);
                }
            } else {
                updates.insert(update.id, event);
            }
        }

        fn receive_batch(
            receiver: &Receiver<UpdateEvent>,
        ) -> HashMap<BufferId, UpdateEvent> {
            let mut updates = HashMap::new();
            loop {
                let update = receiver.recv().unwrap();
                insert_update(&mut updates, update);
                match receiver.try_recv() {
                    Ok(update) => {
                        insert_update(&mut updates, update);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => (),
                }
            }
            updates
        }

        let mut highlighter = Highlighter::new();
        let mut highlight_configs = HashMap::new();
        loop {
            let events = receive_batch(&receiver);
            for (_, event) in events {
                let (update, tokens) = match event {
                    UpdateEvent::Buffer(update) => (update, None),
                    UpdateEvent::SemanticTokens(update, tokens) => {
                        (update, Some(tokens))
                    }
                };

                let semantic_tokens = tokens.is_some();

                let highlights = if let Some(tokens) = tokens {
                    let start = std::time::SystemTime::now();
                    let mut highlights = SpansBuilder::new(update.rope.len());
                    for (start, end, hl) in tokens {
                        highlights.add_span(
                            Interval::new(start, end),
                            Style {
                                fg_color: Some(hl.to_string()),
                            },
                        );
                    }
                    let highlights = highlights.build();
                    let end = std::time::SystemTime::now();
                    let duration = end.duration_since(start).unwrap().as_micros();
                    // println!("semantic tokens took {}", duration);
                    highlights
                } else {
                    if !highlight_configs.contains_key(&update.language) {
                        let (highlight_config, highlight_names) =
                            new_highlight_config(update.language);
                        highlight_configs.insert(
                            update.language,
                            (highlight_config, highlight_names),
                        );
                    }
                    let (highlight_config, highlight_names) =
                        highlight_configs.get(&update.language).unwrap();
                    let mut current_hl: Option<Highlight> = None;
                    let mut highlights = SpansBuilder::new(update.rope.len());
                    for hightlight in highlighter
                        .highlight(
                            highlight_config,
                            update
                                .rope
                                .slice_to_cow(0..update.rope.len())
                                .as_bytes(),
                            None,
                            |_| None,
                        )
                        .unwrap()
                    {
                        if let Ok(highlight) = hightlight {
                            match highlight {
                                HighlightEvent::Source { start, end } => {
                                    if let Some(hl) = current_hl {
                                        if let Some(hl) = highlight_names.get(hl.0) {
                                            highlights.add_span(
                                                Interval::new(start, end),
                                                Style {
                                                    fg_color: Some(hl.to_string()),
                                                },
                                            );
                                        }
                                    }
                                }
                                HighlightEvent::HighlightStart(hl) => {
                                    current_hl = Some(hl);
                                }
                                HighlightEvent::HighlightEnd => current_hl = None,
                            }
                        }
                    }
                    let highlights = highlights.build();
                    highlights
                };

                event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateStyle {
                        id: update.id,
                        path: update.path,
                        rev: update.rev,
                        highlights,
                        semantic_tokens,
                    },
                    Target::Widget(tab_id),
                );
            }
        }
    }
}

pub struct LapceTabLens(pub WidgetId);

impl Lens<LapceWindowData, LapceTabData> for LapceTabLens {
    fn with<V, F: FnOnce(&LapceTabData) -> V>(
        &self,
        data: &LapceWindowData,
        f: F,
    ) -> V {
        let tab = data.tabs.get(&self.0).unwrap();
        f(&tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceTabData) -> V>(
        &self,
        data: &mut LapceWindowData,
        f: F,
    ) -> V {
        let mut tab = data.tabs.get(&self.0).unwrap().clone();
        tab.keypress = data.keypress.clone();
        tab.theme = data.theme.clone();
        let result = f(&mut tab);
        data.keypress = tab.keypress.clone();
        data.theme = tab.theme.clone();
        if !tab.same(data.tabs.get(&self.0).unwrap()) {
            data.tabs.insert(self.0, tab);
        }
        result
    }
}

pub struct LapceWindowLens(pub WindowId);

impl Lens<LapceData, LapceWindowData> for LapceWindowLens {
    fn with<V, F: FnOnce(&LapceWindowData) -> V>(
        &self,
        data: &LapceData,
        f: F,
    ) -> V {
        let tab = data.windows.get(&self.0).unwrap();
        f(&tab)
    }

    fn with_mut<V, F: FnOnce(&mut LapceWindowData) -> V>(
        &self,
        data: &mut LapceData,
        f: F,
    ) -> V {
        let mut win = data.windows.get(&self.0).unwrap().clone();
        win.keypress = data.keypress.clone();
        win.theme = data.theme.clone();
        let result = f(&mut win);
        data.keypress = win.keypress.clone();
        data.theme = win.theme.clone();
        if !win.same(data.windows.get(&self.0).unwrap()) {
            data.windows.insert(self.0, win);
        }
        result
    }
}

#[derive(Clone, Default)]
pub struct RegisterData {
    pub content: String,
    pub mode: VisualMode,
}

#[derive(Clone, Default)]
pub struct Register {
    unamed: RegisterData,
    last_yank: RegisterData,
    last_deletes: [RegisterData; 10],
    newest_delete: usize,
}

impl Register {
    pub fn add_delete(&mut self, data: RegisterData) {
        self.unamed = data.clone();
    }

    pub fn add_yank(&mut self, data: RegisterData) {
        self.unamed = data.clone();
        self.last_yank = data;
    }
}

#[derive(Clone, Debug)]
pub enum EditorKind {
    PalettePreview,
    SplitActive,
}

#[derive(Clone, Data, Lens)]
pub struct LapceMainSplitData {
    pub split_id: Arc<WidgetId>,
    pub active: Arc<WidgetId>,
    pub editors: im::HashMap<WidgetId, Arc<LapceEditorData>>,
    // pub buffers: im::HashMap<BufferId, Arc<BufferNew>>,
    pub open_files: im::HashMap<PathBuf, Arc<BufferNew>>,
    pub update_sender: Arc<Sender<UpdateEvent>>,
    pub register: Arc<Register>,
    pub proxy: Arc<LapceProxy>,
    pub palette_preview_editor: Arc<WidgetId>,
}

impl LapceMainSplitData {
    pub fn notify_update_text_layouts(&self, ctx: &mut EventCtx, path: &PathBuf) {
        for (editor_id, editor) in &self.editors {
            if &editor.buffer == path {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FillTextLayouts,
                    Target::Widget(*editor_id),
                ));
            }
        }
    }

    pub fn editor_kind(&self, kind: &EditorKind) -> &LapceEditorData {
        match kind {
            EditorKind::PalettePreview => {
                self.editors.get(&self.palette_preview_editor).unwrap()
            }
            EditorKind::SplitActive => self.editors.get(&self.active).unwrap(),
        }
    }

    pub fn editor_kind_mut(&mut self, kind: &EditorKind) -> &mut LapceEditorData {
        match kind {
            EditorKind::PalettePreview => Arc::make_mut(
                self.editors.get_mut(&self.palette_preview_editor).unwrap(),
            ),
            EditorKind::SplitActive => {
                Arc::make_mut(self.editors.get_mut(&self.active).unwrap())
            }
        }
    }

    pub fn active_editor(&self) -> &LapceEditorData {
        self.editors.get(&self.active).unwrap()
    }

    pub fn active_editor_mut(&mut self) -> &mut LapceEditorData {
        Arc::make_mut(self.editors.get_mut(&self.active).unwrap())
    }

    pub fn jump_to_position(
        &mut self,
        ctx: &mut EventCtx,
        kind: &EditorKind,
        position: Position,
    ) {
        let editor = self.editor_kind_mut(kind);
        let location = EditorLocationNew {
            path: editor.buffer.clone(),
            position,
            scroll_offset: None,
        };
        self.jump_to_location(ctx, kind, location);
    }

    pub fn jump_to_location(
        &mut self,
        ctx: &mut EventCtx,
        kind: &EditorKind,
        location: EditorLocationNew,
    ) {
        let path = self.editor_kind(kind).buffer.clone();
        let buffer = self.open_files.get(&path).unwrap().clone();
        let editor = self.editor_kind_mut(kind);
        editor.save_jump_location(&buffer);
        let editor_view_id = editor.view_id;
        self.go_to_location(ctx, editor_view_id, location);
    }

    pub fn go_to_location(
        &mut self,
        ctx: &mut EventCtx,
        editor_view_id: WidgetId,
        location: EditorLocationNew,
    ) {
        let editor = Arc::make_mut(self.editors.get_mut(&editor_view_id).unwrap());
        let new_buffer = editor.buffer != location.path;
        let path = location.path.clone();
        let buffer_exists = self.open_files.contains_key(&path);
        if !buffer_exists {
            let buffer =
                Arc::new(BufferNew::new(path.clone(), self.update_sender.clone()));
            self.open_files.insert(path.clone(), buffer.clone());
            buffer.retrieve_file_and_go_to_location(
                self.proxy.clone(),
                ctx.get_external_handle(),
                editor.view_id,
                location,
            );
        } else {
            let buffer = self.open_files.get(&path).unwrap();
            let offset = buffer.offset_of_position(&location.position);
            editor.buffer = path.clone();
            editor.cursor = Cursor::new(CursorMode::Normal(offset), None);

            if let Some(scroll_offset) = location.scroll_offset {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ForceScrollTo(scroll_offset.x, scroll_offset.y),
                    Target::Widget(editor.container_id),
                ));
            } else {
                if new_buffer {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EnsureCursorCenter,
                        Target::Widget(editor.container_id),
                    ));
                } else {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::EnsureCursorVisible(Some(
                            EnsureVisiblePosition::CenterOfWindow,
                        )),
                        Target::Widget(editor.container_id),
                    ));
                }
            }
        }
    }

    pub fn jump_to_line(
        &mut self,
        ctx: &mut EventCtx,
        kind: &EditorKind,
        line: usize,
    ) {
        let buffer = self.open_files.get(&self.editor_kind(kind).buffer).unwrap();
        let offset = buffer.first_non_blank_character_on_line(if line > 0 {
            line - 1
        } else {
            0
        });

        let editor = self.editor_kind_mut(kind);
        editor.cursor = Cursor::new(CursorMode::Normal(offset), None);

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureCursorCenter,
            Target::Widget(editor.container_id),
        ));
    }

    pub fn open_file(&mut self, ctx: &mut EventCtx, path: &PathBuf) {
        let (cursor_offset, scroll_offset) = if let Some(buffer) =
            self.open_files.get(path)
        {
            (buffer.cursor_offset, buffer.scroll_offset)
        } else {
            let buffer =
                Arc::new(BufferNew::new(path.clone(), self.update_sender.clone()));
            self.open_files.insert(path.clone(), buffer.clone());
            buffer.retrieve_file(self.proxy.clone(), ctx.get_external_handle());
            (0, Vec2::new(0.0, 0.0))
        };

        let editor = self.active_editor_mut();
        editor.buffer = path.clone();
        editor.cursor = Cursor::new(CursorMode::Normal(cursor_offset), None);

        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ForceScrollTo(scroll_offset.x, scroll_offset.y),
            Target::Widget(editor.container_id),
        ));
    }
}

impl LapceMainSplitData {
    pub fn new(
        palette_preview_editor: WidgetId,
        update_sender: Arc<Sender<UpdateEvent>>,
        proxy: Arc<LapceProxy>,
    ) -> Self {
        let split_id = Arc::new(WidgetId::next());
        let mut editors = im::HashMap::new();
        let path = PathBuf::from("/Users/Lulu/lapce/core/src/editor.rs");
        let editor = LapceEditorData::new(None, *split_id, path.clone());
        let view_id = editor.view_id;
        editors.insert(editor.view_id, Arc::new(editor));
        let buffer = BufferNew::new(path.clone(), update_sender.clone());
        let mut open_files = im::HashMap::new();
        open_files.insert(path.clone(), Arc::new(buffer));

        let path = PathBuf::from("[Palette Preview Editor]");
        let editor = LapceEditorData::new(
            Some(palette_preview_editor),
            *split_id,
            path.clone(),
        );
        editors.insert(editor.view_id, Arc::new(editor));
        let mut buffer = BufferNew::new(path.clone(), update_sender.clone());
        buffer.loaded = true;
        open_files.insert(path.clone(), Arc::new(buffer));

        Self {
            split_id,
            editors,
            // buffers,
            open_files,
            active: Arc::new(view_id),
            update_sender,
            register: Arc::new(Register::default()),
            proxy,
            palette_preview_editor: Arc::new(palette_preview_editor),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LapceEditorData {
    pub split_id: WidgetId,
    pub view_id: WidgetId,
    pub container_id: WidgetId,
    pub editor_id: WidgetId,
    pub buffer: PathBuf,
    pub scroll_offset: Vec2,
    pub cursor: Cursor,
    pub size: Size,
    pub window_origin: Point,
    pub snippet: Option<Vec<(usize, (usize, usize))>>,
    pub locations: Vec<EditorLocationNew>,
    pub current_location: usize,
    last_movement: Movement,
}

impl LapceEditorData {
    pub fn new(
        view_id: Option<WidgetId>,
        split_id: WidgetId,
        buffer: PathBuf,
    ) -> Self {
        Self {
            split_id,
            view_id: view_id.unwrap_or(WidgetId::next()),
            container_id: WidgetId::next(),
            editor_id: WidgetId::next(),
            buffer,
            scroll_offset: Vec2::ZERO,
            cursor: Cursor::default(),
            size: Size::ZERO,
            window_origin: Point::ZERO,
            snippet: None,
            locations: vec![],
            current_location: 0,
            last_movement: Movement::Left,
        }
    }

    pub fn add_snippet_placeholders(
        &mut self,
        new_placeholders: Vec<(usize, (usize, usize))>,
    ) {
        if self.snippet.is_none() {
            if new_placeholders.len() > 1 {
                self.snippet = Some(new_placeholders);
            }
            return;
        }

        let placeholders = self.snippet.as_mut().unwrap();

        let mut current = 0;
        let offset = self.cursor.offset();
        for (i, (_, (start, end))) in placeholders.iter().enumerate() {
            if *start <= offset && offset <= *end {
                current = i;
                break;
            }
        }

        let v = placeholders.split_off(current);
        placeholders.extend_from_slice(&new_placeholders);
        placeholders.extend_from_slice(&v[1..]);
    }

    pub fn save_jump_location(&mut self, buffer: &BufferNew) {
        let location = EditorLocationNew {
            path: buffer.path.clone(),
            position: buffer.offset_to_position(self.cursor.offset()),
            scroll_offset: Some(self.scroll_offset.clone()),
        };
        self.locations.push(location);
        self.current_location = self.locations.len();
    }
}

#[derive(Clone, Data, Lens)]
pub struct LapceEditorViewData {
    pub main_split: LapceMainSplitData,
    pub proxy: Arc<LapceProxy>,
    pub editor: Arc<LapceEditorData>,
    pub buffer: Arc<BufferNew>,
    pub keypress: Arc<KeyPressData>,
    pub completion: Arc<CompletionData>,
    pub palette: Arc<WidgetId>,
    pub theme: Arc<std::collections::HashMap<String, Color>>,
}

impl LapceEditorViewData {
    pub fn key_down(
        &mut self,
        ctx: &mut EventCtx,
        key_event: &KeyEvent,
        env: &Env,
    ) -> bool {
        let mut keypress = self.keypress.clone();
        let k = Arc::make_mut(&mut keypress);
        let executed = k.key_down(ctx, key_event, self, env);
        self.keypress = keypress;
        executed
    }

    pub fn buffer_mut(&mut self) -> &mut BufferNew {
        Arc::make_mut(&mut self.buffer)
    }

    pub fn sync_buffer_position(&mut self, scroll_offset: Vec2) {
        let cursor_offset = self.editor.cursor.offset();
        let buffer = self.buffer_mut();
        buffer.cursor_offset = cursor_offset;
        buffer.scroll_offset = scroll_offset;
    }

    pub fn fill_text_layouts(
        &mut self,
        ctx: &mut EventCtx,
        theme: &Arc<HashMap<String, Color>>,
        env: &Env,
    ) {
        let start = std::time::SystemTime::now();
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let start_line = (self.editor.scroll_offset.y / line_height) as usize;
        let size = self.editor.size;
        let num_lines = ((size.height / line_height).ceil()) as usize;
        let text = ctx.text();
        let buffer = self.buffer_mut();
        for line in start_line..start_line + num_lines + 1 {
            buffer.update_line_layouts(text, line, theme, env);
        }
        let end = std::time::SystemTime::now();
        let duration = end.duration_since(start).unwrap().as_micros();
        // println!("fill text layout took {}", duration);
    }

    fn move_command(
        &self,
        count: Option<usize>,
        cmd: &LapceCommand,
    ) -> Option<Movement> {
        match cmd {
            LapceCommand::Left => Some(Movement::Left),
            LapceCommand::Right => Some(Movement::Right),
            LapceCommand::Up => Some(Movement::Up),
            LapceCommand::Down => Some(Movement::Down),
            LapceCommand::LineStart => Some(Movement::StartOfLine),
            LapceCommand::LineEnd => Some(Movement::EndOfLine),
            LapceCommand::GotoLineDefaultFirst => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::First),
            }),
            LapceCommand::GotoLineDefaultLast => Some(match count {
                Some(n) => Movement::Line(LinePosition::Line(n)),
                None => Movement::Line(LinePosition::Last),
            }),
            LapceCommand::WordBackward => Some(Movement::WordBackward),
            LapceCommand::WordFoward => Some(Movement::WordForward),
            LapceCommand::WordEndForward => Some(Movement::WordEndForward),
            LapceCommand::MatchPairs => Some(Movement::MatchPairs),
            LapceCommand::NextUnmatchedRightBracket => {
                Some(Movement::NextUnmatched(')'))
            }
            LapceCommand::PreviousUnmatchedLeftBracket => {
                Some(Movement::PreviousUnmatched('('))
            }
            LapceCommand::NextUnmatchedRightCurlyBracket => {
                Some(Movement::NextUnmatched('}'))
            }
            LapceCommand::PreviousUnmatchedLeftCurlyBracket => {
                Some(Movement::PreviousUnmatched('{'))
            }
            _ => None,
        }
    }

    fn toggle_visual(&mut self, visual_mode: VisualMode) {
        let cursor = &mut Arc::make_mut(&mut self.editor).cursor;

        match &cursor.mode {
            CursorMode::Visual { start, end, mode } => {
                if mode != &visual_mode {
                    cursor.mode = CursorMode::Visual {
                        start: *start,
                        end: *end,
                        mode: visual_mode,
                    };
                } else {
                    cursor.mode = CursorMode::Normal(*end);
                };
            }
            _ => {
                let offset = cursor.offset();
                cursor.mode = CursorMode::Visual {
                    start: offset,
                    end: offset,
                    mode: visual_mode,
                };
            }
        }
    }

    pub fn apply_completion_item(
        &mut self,
        ctx: &mut EventCtx,
        item: &CompletionItem,
    ) -> Result<()> {
        let additioal_edit = item.additional_text_edits.as_ref().map(|edits| {
            edits
                .iter()
                .map(|edit| {
                    let selection = Selection::region(
                        self.buffer.offset_of_position(&edit.range.start),
                        self.buffer.offset_of_position(&edit.range.end),
                    );
                    (selection, edit.new_text.clone())
                })
                .collect::<Vec<(Selection, String)>>()
        });
        let additioal_edit = additioal_edit.as_ref().map(|edits| {
            edits
                .into_iter()
                .map(|(selection, c)| (selection, c.as_str()))
                .collect()
        });

        let text_format = item
            .insert_text_format
            .unwrap_or(lsp_types::InsertTextFormat::PlainText);
        if let Some(edit) = &item.text_edit {
            match edit {
                CompletionTextEdit::Edit(edit) => {
                    let offset = self.editor.cursor.offset();
                    let start_offset = self.buffer.prev_code_boundary(offset);
                    let end_offset = self.buffer.next_code_boundary(offset);
                    let edit_start =
                        self.buffer.offset_of_position(&edit.range.start);
                    let edit_end = self.buffer.offset_of_position(&edit.range.end);
                    let selection = Selection::region(
                        start_offset.min(edit_start),
                        end_offset.max(edit_end),
                    );
                    match text_format {
                        lsp_types::InsertTextFormat::PlainText => {
                            let (selection, _) = self.edit(
                                ctx,
                                &selection,
                                &edit.new_text,
                                additioal_edit,
                                true,
                                EditType::InsertChars,
                            );
                            self.set_cursor_after_change(selection);
                            return Ok(());
                        }
                        lsp_types::InsertTextFormat::Snippet => {
                            let snippet = Snippet::from_str(&edit.new_text)?;
                            let text = snippet.text();
                            let (selection, delta) = self.edit(
                                ctx,
                                &selection,
                                &text,
                                additioal_edit,
                                true,
                                EditType::InsertChars,
                            );

                            let mut transformer = Transformer::new(&delta);
                            let offset = transformer
                                .transform(start_offset.min(edit_start), false);
                            let snippet_tabs = snippet.tabs(offset);

                            if snippet_tabs.len() == 0 {
                                self.set_cursor_after_change(selection);
                                return Ok(());
                            }

                            let mut selection = Selection::new();
                            let (tab, (start, end)) = &snippet_tabs[0];
                            let region = SelRegion::new(*start, *end, None);
                            selection.add_region(region);
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                            Arc::make_mut(&mut self.editor)
                                .add_snippet_placeholders(snippet_tabs);
                            return Ok(());
                        }
                    }
                }
                CompletionTextEdit::InsertAndReplace(_) => (),
            }
        }

        let offset = self.editor.cursor.offset();
        let start_offset = self.buffer.prev_code_boundary(offset);
        let end_offset = self.buffer.next_code_boundary(offset);
        let selection = Selection::region(start_offset, end_offset);

        let (selection, _) = self.edit(
            ctx,
            &selection,
            item.insert_text.as_ref().unwrap_or(&item.label),
            additioal_edit,
            true,
            EditType::InsertChars,
        );
        self.set_cursor_after_change(selection);
        Ok(())
    }

    fn scroll(&mut self, ctx: &mut EventCtx, down: bool, count: usize, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let diff = line_height * count as f64;
        let diff = if down { diff } else { -diff };

        let offset = self.editor.cursor.offset();
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let top = self.editor.scroll_offset.y + diff;
        let bottom = top + self.editor.size.height;

        let line = if (line + 1) as f64 * line_height + line_height > bottom {
            let line = (bottom / line_height).floor() as usize;
            if line > 2 {
                line - 2
            } else {
                0
            }
        } else if line as f64 * line_height - line_height < top {
            let line = (top / line_height).ceil() as usize;
            line + 1
        } else {
            line
        };

        let offset = self.buffer.offset_of_line(line)
            + col.min(self.buffer.line_end_col(line, false));
        self.set_cursor(Cursor::new(
            CursorMode::Normal(offset),
            self.editor.cursor.horiz.clone(),
        ));
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::ScrollTo((self.editor.scroll_offset.x, top)),
            Target::Widget(self.editor.container_id),
        ));
    }

    fn page_move(&mut self, ctx: &mut EventCtx, down: bool, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let lines = (self.editor.size.height / line_height / 2.0).round() as usize;
        let distance = (lines as f64) * line_height;
        let offset = self.editor.cursor.offset();
        let (offset, horiz) = self.buffer.move_offset(
            offset,
            self.editor.cursor.horiz.as_ref(),
            lines,
            if down { &Movement::Down } else { &Movement::Up },
            self.get_mode(),
        );
        self.set_cursor(Cursor::new(CursorMode::Normal(offset), Some(horiz)));
        let rect = Rect::ZERO
            .with_origin(
                self.editor.scroll_offset.to_point()
                    + Vec2::new(0.0, if down { distance } else { -distance }),
            )
            .with_size(self.editor.size.clone());
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::EnsureRectVisible(rect),
            Target::Widget(self.editor.container_id),
        ));
    }

    pub fn jump_location_forward(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) -> Option<()> {
        if self.editor.current_location >= self.editor.locations.len() - 1 {
            return None;
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.current_location += 1;
        let location = editor.locations[editor.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocationNew(editor.view_id, location),
            Target::Auto,
        ));
        None
    }

    pub fn jump_location_backward(
        &mut self,
        ctx: &mut EventCtx,
        env: &Env,
    ) -> Option<()> {
        if self.editor.current_location < 1 {
            return None;
        }
        if self.editor.current_location >= self.editor.locations.len() {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer);
            editor.current_location -= 1;
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.current_location -= 1;
        let location = editor.locations[editor.current_location].clone();
        ctx.submit_command(Command::new(
            LAPCE_UI_COMMAND,
            LapceUICommand::GoToLocationNew(editor.view_id, location),
            Target::Auto,
        ));
        None
    }

    pub fn do_move(&mut self, movement: &Movement, count: usize) {
        if movement.is_jump() && movement != &self.editor.last_movement {
            let editor = Arc::make_mut(&mut self.editor);
            editor.save_jump_location(&self.buffer);
        }
        let editor = Arc::make_mut(&mut self.editor);
        editor.last_movement = movement.clone();
        match &self.editor.cursor.mode {
            &CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    offset,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Normal,
                );
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Normal(new_offset);
                editor.cursor.horiz = Some(horiz);
            }
            CursorMode::Visual { start, end, mode } => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    *end,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    Mode::Visual,
                );
                let start = *start;
                let mode = mode.clone();
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Visual {
                    start,
                    end: new_offset,
                    mode,
                };
                editor.cursor.horiz = Some(horiz);
            }
            CursorMode::Insert(selection) => {
                let selection = self.buffer.update_selection(
                    selection,
                    count,
                    movement,
                    Mode::Insert,
                    false,
                );
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    pub fn cusor_region(&self, env: &Env) -> Rect {
        self.editor.cursor.region(&self.buffer, env)
    }

    pub fn insert_new_line(&mut self, ctx: &mut EventCtx, offset: usize) {
        let line = self.buffer.line_of_offset(offset);
        let line_indent = self.buffer.indent_on_line(line);
        let line_content = self.buffer.offset_line_content(offset);

        let indent = if has_unmatched_pair(&line_content) {
            format!("{}    ", line_indent)
        } else {
            let next_line_indent = self.buffer.indent_on_line(line + 1);
            if next_line_indent.len() > line_indent.len() {
                next_line_indent
            } else {
                line_indent.clone()
            }
        };

        let selection = Selection::caret(offset);
        let content = format!("{}{}", "\n", indent);

        let (selection, _) = self.edit(
            ctx,
            &selection,
            &content,
            None,
            true,
            EditType::InsertNewline,
        );
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor.mode = CursorMode::Insert(selection);
        editor.cursor.horiz = None;
    }

    fn set_cursor_after_change(&mut self, selection: Selection) {
        match self.editor.cursor.mode {
            CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                let offset = selection.min_offset();
                let offset = self.buffer.offset_line_end(offset, false).min(offset);
                self.set_cursor(Cursor::new(CursorMode::Normal(offset), None));
            }
            CursorMode::Insert(_) => {
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
        }
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = cursor;
    }

    fn paste(&mut self, ctx: &mut EventCtx, data: &RegisterData) {
        match data.mode {
            VisualMode::Normal => {
                Arc::make_mut(&mut self.editor).snippet = None;
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line_end = self.buffer.offset_line_end(offset, true);
                        let offset = (offset + 1).min(line_end);
                        Selection::caret(offset)
                    }
                    CursorMode::Insert { .. } | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                };
                let after = !data.content.contains("\n");
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &data.content,
                    None,
                    after,
                    EditType::InsertChars,
                );
                if !after {
                    self.set_cursor_after_change(selection);
                } else {
                    match self.editor.cursor.mode {
                        CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                            let offset = self.buffer.prev_grapheme_offset(
                                selection.min_offset(),
                                1,
                                0,
                            );
                            self.set_cursor(Cursor::new(
                                CursorMode::Normal(offset),
                                None,
                            ));
                        }
                        CursorMode::Insert { .. } => {
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                        }
                    }
                }
            }
            VisualMode::Linewise | VisualMode::Blockwise => {
                let (selection, content) = match &self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let line = self.buffer.line_of_offset(*offset);
                        let offset = self.buffer.offset_of_line(line + 1);
                        (Selection::caret(offset), data.content.clone())
                    }
                    CursorMode::Insert { .. } => (
                        self.editor.cursor.edit_selection(&self.buffer),
                        "\n".to_string() + &data.content,
                    ),
                    CursorMode::Visual { mode, .. } => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let data = match mode {
                            VisualMode::Linewise => data.content.clone(),
                            _ => "\n".to_string() + &data.content,
                        };
                        (selection, data)
                    }
                };
                let (selection, _) = self.edit(
                    ctx,
                    &selection,
                    &content,
                    None,
                    false,
                    EditType::InsertChars,
                );
                match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        let offset = selection.min_offset();
                        let offset = if self.editor.cursor.is_visual() {
                            offset + 1
                        } else {
                            offset
                        };
                        let line = self.buffer.line_of_offset(offset);
                        let offset =
                            self.buffer.first_non_blank_character_on_line(line);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                }
            }
        }
    }

    fn edit(
        &mut self,
        ctx: &mut EventCtx,
        selection: &Selection,
        c: &str,
        additional_edit: Option<Vec<(&Selection, &str)>>,
        after: bool,
        edit_type: EditType,
    ) -> (Selection, RopeDelta) {
        match &self.editor.cursor.mode {
            CursorMode::Normal(_) => {
                if !selection.is_caret() {
                    let data = self.editor.cursor.yank(&self.buffer);
                    let register = Arc::make_mut(&mut self.main_split.register);
                    register.add_delete(data);
                }
            }
            CursorMode::Visual { start, end, mode } => {
                let data = self.editor.cursor.yank(&self.buffer);
                let register = Arc::make_mut(&mut self.main_split.register);
                register.add_delete(data);
            }
            CursorMode::Insert(_) => {}
        }

        let proxy = self.proxy.clone();
        let buffer = self.buffer_mut();
        let delta = if let Some(additional_edit) = additional_edit {
            let mut edits = vec![(selection, c)];
            edits.extend_from_slice(&additional_edit);
            buffer.edit_multiple(ctx, edits, proxy, edit_type)
        } else {
            buffer.edit(ctx, &selection, c, proxy, edit_type)
        };
        self.main_split
            .notify_update_text_layouts(ctx, &self.editor.buffer);
        self.inactive_apply_delta(&delta);
        let selection = selection.apply_delta(&delta, after, InsertDrift::Default);
        if let Some(snippet) = self.editor.snippet.clone() {
            let mut transformer = Transformer::new(&delta);
            Arc::make_mut(&mut self.editor).snippet = Some(
                snippet
                    .iter()
                    .map(|(tab, (start, end))| {
                        (
                            *tab,
                            (
                                transformer.transform(*start, false),
                                transformer.transform(*end, true),
                            ),
                        )
                    })
                    .collect(),
            );
        }
        (selection, delta)
    }

    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        let open_files = self.main_split.open_files.clone();
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id {
                if self.editor.buffer == editor.buffer {
                    Arc::make_mut(editor).cursor.apply_delta(delta);
                }
            }
        }
    }

    pub fn cancel_completion(&mut self) {
        let completion = Arc::make_mut(&mut self.completion);
        completion.cancel();
    }

    fn update_completion(&mut self, ctx: &mut EventCtx) {
        if self.get_mode() != Mode::Insert {
            return;
        }
        let offset = self.editor.cursor.offset();
        let start_offset = self.buffer.prev_code_boundary(offset);
        let end_offset = self.buffer.next_code_boundary(offset);
        let input = self
            .buffer
            .slice_to_cow(start_offset..end_offset)
            .to_string();
        let char = self
            .buffer
            .slice_to_cow(start_offset - 1..start_offset)
            .to_string();
        let completion = Arc::make_mut(&mut self.completion);
        if input == "" && char != "." && char != ":" {
            completion.cancel();
            return;
        }

        if completion.status != CompletionStatus::Inactive
            && completion.offset == start_offset
            && completion.buffer_id == self.buffer.id
        {
            completion.update_input(input.clone());

            if input != "" && !completion.input_items.contains_key(&input) {
                let event_sink = ctx.get_external_handle();
                completion.request(
                    self.proxy.clone(),
                    completion.request_id,
                    self.buffer.id,
                    input,
                    self.buffer.offset_to_position(offset),
                    completion.id,
                    event_sink,
                );
            }

            return;
        }

        completion.buffer_id = self.buffer.id;
        completion.offset = start_offset;
        completion.input = input.clone();
        completion.status = CompletionStatus::Started;
        completion.items = Arc::new(Vec::new());
        completion.input_items.clear();
        completion.request_id += 1;
        let event_sink = ctx.get_external_handle();
        completion.request(
            self.proxy.clone(),
            completion.request_id,
            self.buffer.id,
            "".to_string(),
            self.buffer.offset_to_position(start_offset),
            completion.id,
            event_sink.clone(),
        );
        if input != "" {
            completion.request(
                self.proxy.clone(),
                completion.request_id,
                self.buffer.id,
                input,
                self.buffer.offset_to_position(offset),
                completion.id,
                event_sink,
            );
        }
    }
}

pub struct LapceEditorLens(pub WidgetId);

impl Lens<LapceTabData, LapceEditorViewData> for LapceEditorLens {
    fn with<V, F: FnOnce(&LapceEditorViewData) -> V>(
        &self,
        data: &LapceTabData,
        f: F,
    ) -> V {
        let main_split = &data.main_split;
        let editor = main_split.editors.get(&self.0).unwrap();
        let editor_view = LapceEditorViewData {
            buffer: main_split.open_files.get(&editor.buffer).unwrap().clone(),
            editor: editor.clone(),
            main_split: main_split.clone(),
            keypress: data.keypress.clone(),
            completion: data.completion.clone(),
            palette: Arc::new(data.palette.widget_id),
            theme: data.theme.clone(),
            proxy: data.proxy.clone(),
        };
        f(&editor_view)
    }

    fn with_mut<V, F: FnOnce(&mut LapceEditorViewData) -> V>(
        &self,
        data: &mut LapceTabData,
        f: F,
    ) -> V {
        let main_split = &data.main_split;
        let editor = main_split.editors.get(&self.0).unwrap().clone();
        let mut editor_view = LapceEditorViewData {
            buffer: main_split.open_files.get(&editor.buffer).unwrap().clone(),
            editor: editor.clone(),
            main_split: data.main_split.clone(),
            keypress: data.keypress.clone(),
            completion: data.completion.clone(),
            palette: Arc::new(data.palette.widget_id),
            theme: data.theme.clone(),
            proxy: data.proxy.clone(),
        };
        let result = f(&mut editor_view);

        data.keypress = editor_view.keypress.clone();
        data.completion = editor_view.completion.clone();
        data.main_split = editor_view.main_split.clone();
        data.theme = editor_view.theme.clone();
        if !editor.same(&editor_view.editor) {
            data.main_split
                .editors
                .insert(self.0, editor_view.editor.clone());
        }
        data.main_split
            .open_files
            .insert(editor_view.buffer.path.clone(), editor_view.buffer.clone());

        result
    }
}

impl KeyPressFocus for LapceEditorViewData {
    fn get_mode(&self) -> Mode {
        match self.editor.cursor.mode {
            CursorMode::Normal(_) => Mode::Normal,
            CursorMode::Visual { .. } => Mode::Visual,
            CursorMode::Insert(_) => Mode::Insert,
        }
    }

    fn check_condition(&self, condition: &str) -> bool {
        match condition {
            "editor_focus" => true,
            "in_snippet" => self.editor.snippet.is_some(),
            "list_focus" => {
                self.completion.status == CompletionStatus::Done
                    && if self.completion.input == "" {
                        self.completion.items.len() > 0
                    } else {
                        self.completion.filtered_items.len() > 0
                    }
            }
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceCommand,
        count: Option<usize>,
        env: &Env,
    ) {
        if let Some(movement) = self.move_command(count, cmd) {
            self.do_move(&movement, count.unwrap_or(1));
            if let Some(snippet) = self.editor.snippet.as_ref() {
                let offset = self.editor.cursor.offset();
                let mut within_region = false;
                for (_, (start, end)) in snippet {
                    if offset >= *start && offset <= *end {
                        within_region = true;
                        break;
                    }
                }
                if !within_region {
                    Arc::make_mut(&mut self.editor).snippet = None;
                }
            }
            self.cancel_completion();
            return;
        }
        match cmd {
            LapceCommand::SplitLeft => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorMove(
                        SplitMoveDirection::Left,
                        self.editor.view_id,
                    ),
                    Target::Widget(self.editor.split_id),
                ));
            }
            LapceCommand::SplitRight => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorMove(
                        SplitMoveDirection::Right,
                        self.editor.view_id,
                    ),
                    Target::Widget(self.editor.split_id),
                ));
            }
            LapceCommand::SplitExchange => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorExchange(self.editor.view_id),
                    Target::Widget(self.editor.split_id),
                ));
            }
            LapceCommand::SplitVertical => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditor(true, self.editor.view_id),
                    Target::Widget(self.editor.split_id),
                ));
            }
            LapceCommand::SplitClose => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::SplitEditorClose(self.editor.view_id),
                    Target::Widget(self.editor.split_id),
                ));
            }
            LapceCommand::Undo => {
                let proxy = self.proxy.clone();
                let buffer = self.buffer_mut();
                if let Some(delta) = buffer.do_undo(proxy) {
                    self.main_split
                        .notify_update_text_layouts(ctx, &self.editor.buffer);
                    let selection = Selection::caret(self.editor.cursor.offset())
                        .apply_delta(&delta, true, InsertDrift::Default);
                    self.set_cursor_after_change(selection);
                }
            }
            LapceCommand::Redo => {
                let proxy = self.proxy.clone();
                let buffer = self.buffer_mut();
                if let Some(delta) = buffer.do_redo(proxy) {
                    self.main_split
                        .notify_update_text_layouts(ctx, &self.editor.buffer);
                    let selection = Selection::caret(self.editor.cursor.offset())
                        .apply_delta(&delta, true, InsertDrift::Default);
                    self.set_cursor_after_change(selection);
                }
            }
            LapceCommand::Append => {
                let offset = self
                    .buffer
                    .move_offset(
                        self.editor.cursor.offset(),
                        None,
                        1,
                        &Movement::Right,
                        Mode::Insert,
                    )
                    .0;
                self.buffer_mut().update_edit_type();
                self.set_cursor(Cursor::new(
                    CursorMode::Insert(Selection::caret(offset)),
                    None,
                ));
            }
            LapceCommand::AppendEndOfLine => {
                let (offset, horiz) = self.buffer.move_offset(
                    self.editor.cursor.offset(),
                    None,
                    1,
                    &Movement::EndOfLine,
                    Mode::Insert,
                );
                self.buffer_mut().update_edit_type();
                self.set_cursor(Cursor::new(
                    CursorMode::Insert(Selection::caret(offset)),
                    Some(horiz),
                ));
            }
            LapceCommand::InsertMode => {
                Arc::make_mut(&mut self.editor).cursor.mode = CursorMode::Insert(
                    Selection::caret(self.editor.cursor.offset()),
                );
                self.buffer_mut().update_edit_type();
            }
            LapceCommand::InsertFirstNonBlank => {
                match &self.editor.cursor.mode {
                    CursorMode::Normal(offset) => {
                        let (offset, horiz) = self.buffer.move_offset(
                            *offset,
                            None,
                            1,
                            &Movement::FirstNonBlank,
                            Mode::Normal,
                        );
                        self.buffer_mut().update_edit_type();
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(Selection::caret(offset)),
                            Some(horiz),
                        ));
                    }
                    CursorMode::Visual { start, end, mode } => {
                        let mut selection = Selection::new();
                        for region in
                            self.editor.cursor.edit_selection(&self.buffer).regions()
                        {
                            selection.add_region(SelRegion::caret(region.min()));
                        }
                        self.buffer_mut().update_edit_type();
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {}
                };
            }
            LapceCommand::NewLineAbove => {
                let line = self.editor.cursor.current_line(&self.buffer);
                let offset = if line > 0 {
                    self.buffer.line_end_offset(line - 1, true)
                } else {
                    self.buffer.first_non_blank_character_on_line(line)
                };
                self.insert_new_line(ctx, offset);
            }
            LapceCommand::NewLineBelow => {
                let offset = self.editor.cursor.offset();
                let offset = self.buffer.offset_line_end(offset, true);
                self.insert_new_line(ctx, offset);
            }
            LapceCommand::DeleteToBeginningOfLine => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let selection = self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::StartOfLine,
                            Mode::Insert,
                            true,
                        );
                        selection
                    }
                };
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        let offset = selection.min_offset();
                        let offset =
                            self.buffer.offset_line_end(offset, false).min(offset);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Insert(_) => {
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }
                }
            }
            LapceCommand::Yank => {
                let data = self.editor.cursor.yank(&self.buffer);
                let register = Arc::make_mut(&mut self.main_split.register);
                register.add_yank(data);
                match &self.editor.cursor.mode {
                    CursorMode::Visual { start, end, mode } => {
                        let offset = *start.min(end);
                        let offset =
                            self.buffer.offset_line_end(offset, false).min(offset);
                        self.set_cursor(Cursor::new(
                            CursorMode::Normal(offset),
                            None,
                        ));
                    }
                    CursorMode::Normal(_) => {}
                    CursorMode::Insert(_) => {}
                }
            }
            LapceCommand::ClipboardCopy => {
                let data = self.editor.cursor.yank(&self.buffer);
                Application::global().clipboard().put_string(data.content);
            }
            LapceCommand::ClipboardPaste => {
                if let Some(s) = Application::global().clipboard().get_string() {
                    let data = RegisterData {
                        content: s.to_string(),
                        mode: VisualMode::Normal,
                    };
                    self.paste(ctx, &data);
                }
            }
            LapceCommand::Paste => {
                let data = self.main_split.register.unamed.clone();
                self.paste(ctx, &data);
            }
            LapceCommand::DeleteWordBackward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let selection = self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::WordBackward,
                            Mode::Insert,
                            true,
                        );
                        selection
                    }
                };
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteBackward => {
                let selection = match self.editor.cursor.mode {
                    CursorMode::Normal(_) | CursorMode::Visual { .. } => {
                        self.editor.cursor.edit_selection(&self.buffer)
                    }
                    CursorMode::Insert(_) => {
                        let selection =
                            self.editor.cursor.edit_selection(&self.buffer);
                        let selection = self.buffer.update_selection(
                            &selection,
                            1,
                            &Movement::Left,
                            Mode::Insert,
                            true,
                        );
                        selection
                    }
                };
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForeward => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor_after_change(selection);
                self.update_completion(ctx);
            }
            LapceCommand::DeleteForewardAndInsert => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                let (selection, _) =
                    self.edit(ctx, &selection, "", None, true, EditType::Delete);
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
                self.update_completion(ctx);
            }
            LapceCommand::InsertNewLine => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                if selection.regions().len() > 1 {
                    let (selection, _) = self.edit(
                        ctx,
                        &selection,
                        "\n",
                        None,
                        true,
                        EditType::InsertNewline,
                    );
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                    return;
                };
                self.insert_new_line(ctx, self.editor.cursor.offset());
                self.update_completion(ctx);
            }
            LapceCommand::ToggleVisualMode => {
                self.toggle_visual(VisualMode::Normal);
            }
            LapceCommand::ToggleLinewiseVisualMode => {
                self.toggle_visual(VisualMode::Linewise);
            }
            LapceCommand::ToggleBlockwiseVisualMode => {
                self.toggle_visual(VisualMode::Blockwise);
            }
            LapceCommand::CenterOfWindow => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::EnsureCursorCenter,
                    Target::Widget(self.editor.container_id),
                ));
            }
            LapceCommand::ScrollDown => {
                self.scroll(ctx, true, count.unwrap_or(1), env);
            }
            LapceCommand::ScrollUp => {
                self.scroll(ctx, false, count.unwrap_or(1), env);
            }
            LapceCommand::PageDown => {
                self.page_move(ctx, true, env);
            }
            LapceCommand::PageUp => {
                self.page_move(ctx, false, env);
            }
            LapceCommand::JumpLocationBackward => {
                self.jump_location_backward(ctx, env);
            }
            LapceCommand::JumpLocationForward => {
                self.jump_location_forward(ctx, env);
            }
            LapceCommand::ListNext => {
                let completion = Arc::make_mut(&mut self.completion);
                completion.next();
            }
            LapceCommand::ListPrevious => {
                let completion = Arc::make_mut(&mut self.completion);
                completion.previous();
            }
            LapceCommand::JumpToNextSnippetPlaceholder => {
                if let Some(snippet) = self.editor.snippet.as_ref() {
                    let mut current = 0;
                    let offset = self.editor.cursor.offset();
                    for (i, (_, (start, end))) in snippet.iter().enumerate() {
                        if *start <= offset && offset <= *end {
                            current = i;
                            break;
                        }
                    }

                    let last_placeholder = current + 1 >= snippet.len() - 1;

                    if let Some((_, (start, end))) = snippet.get(current + 1) {
                        let mut selection = Selection::new();
                        let region = SelRegion::new(*start, *end, None);
                        selection.add_region(region);
                        self.set_cursor(Cursor::new(
                            CursorMode::Insert(selection),
                            None,
                        ));
                    }

                    if last_placeholder {
                        Arc::make_mut(&mut self.editor).snippet = None;
                    }
                    self.cancel_completion();
                }
            }
            LapceCommand::JumpToPrevSnippetPlaceholder => {
                if let Some(snippet) = self.editor.snippet.as_ref() {
                    let mut current = 0;
                    let offset = self.editor.cursor.offset();
                    for (i, (_, (start, end))) in snippet.iter().enumerate() {
                        if *start <= offset && offset <= *end {
                            current = i;
                            break;
                        }
                    }

                    if current > 0 {
                        if let Some((_, (start, end))) = snippet.get(current - 1) {
                            let mut selection = Selection::new();
                            let region = SelRegion::new(*start, *end, None);
                            selection.add_region(region);
                            self.set_cursor(Cursor::new(
                                CursorMode::Insert(selection),
                                None,
                            ));
                        }
                        self.cancel_completion();
                    }
                }
            }
            LapceCommand::ListSelect => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);

                let count = self.completion.input.len();
                let selection = if count > 0 {
                    self.buffer.update_selection(
                        &selection,
                        count,
                        &Movement::Left,
                        Mode::Insert,
                        true,
                    )
                } else {
                    selection
                };

                let item = self.completion.current_item().to_owned();
                self.cancel_completion();
                if item.data.is_some() {
                    let container_id = self.editor.container_id;
                    let buffer_id = self.buffer.id;
                    let rev = self.buffer.rev;
                    let offset = self.editor.cursor.offset();
                    let event_sink = ctx.get_external_handle();
                    self.proxy.completion_resolve(
                        buffer_id,
                        item.clone(),
                        Box::new(move |result| {
                            println!("completion resolve result {:?}", result);
                            let mut item = item.clone();
                            if let Ok(res) = result {
                                if let Ok(i) =
                                    serde_json::from_value::<CompletionItem>(res)
                                {
                                    item = i;
                                }
                            };
                            event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::ResolveCompletion(
                                    buffer_id, rev, offset, item,
                                ),
                                Target::Widget(container_id),
                            );
                        }),
                    );
                } else {
                    self.apply_completion_item(ctx, &item);
                }
            }
            LapceCommand::NormalMode => {
                let offset = match &self.editor.cursor.mode {
                    CursorMode::Insert(selection) => {
                        self.buffer
                            .move_offset(
                                selection.get_cursor_offset(),
                                None,
                                1,
                                &Movement::Left,
                                Mode::Normal,
                            )
                            .0
                    }
                    CursorMode::Visual { start, end, mode } => {
                        self.buffer.offset_line_end(*end, false).min(*end)
                    }
                    CursorMode::Normal(offset) => *offset,
                };
                self.buffer_mut().update_edit_type();
                let mut cursor = &mut Arc::make_mut(&mut self.editor).cursor;
                cursor.mode = CursorMode::Normal(offset);
                cursor.horiz = None;
                Arc::make_mut(&mut self.editor).snippet = None;
                self.cancel_completion();
            }
            LapceCommand::GotoDefinition => {
                let offset = self.editor.cursor.offset();
                let start_offset = self.buffer.prev_code_boundary(offset);
                let start_position = self.buffer.offset_to_position(start_offset);
                let event_sink = ctx.get_external_handle();
                let buffer_id = self.buffer.id;
                let position = self.buffer.offset_to_position(offset);
                let proxy = self.proxy.clone();
                self.proxy.get_definition(
                    offset,
                    buffer_id,
                    position,
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            if let Ok(resp) =
                                serde_json::from_value::<GotoDefinitionResponse>(res)
                            {
                                if let Some(location) = match resp {
                                    GotoDefinitionResponse::Scalar(location) => {
                                        Some(location)
                                    }
                                    GotoDefinitionResponse::Array(locations) => {
                                        if locations.len() > 0 {
                                            Some(locations[0].clone())
                                        } else {
                                            None
                                        }
                                    }
                                    GotoDefinitionResponse::Link(location_links) => {
                                        None
                                    }
                                } {
                                    if location.range.start == start_position {
                                        proxy.get_references(
                                            buffer_id,
                                            position,
                                            Box::new(move |result| {
                                                process_get_references(
                                                    offset, result, event_sink,
                                                );
                                            }),
                                        );
                                    } else {
                                        event_sink.submit_command(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::GotoDefinition(
                                                offset,
                                                EditorLocationNew {
                                                    path: PathBuf::from(
                                                        location.uri.path(),
                                                    ),
                                                    position: location.range.start,
                                                    scroll_offset: None,
                                                },
                                            ),
                                            Target::Auto,
                                        );
                                    }
                                }
                            }
                        }
                    }),
                );
            }
            LapceCommand::Palette => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(None),
                    Target::Widget(*self.palette),
                ));
            }
            LapceCommand::PaletteSymbol => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::DocumentSymbol)),
                    Target::Widget(*self.palette),
                ));
            }
            LapceCommand::PaletteLine => {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::RunPalette(Some(PaletteType::Line)),
                    Target::Widget(*self.palette),
                ));
            }
            _ => (),
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.get_mode() == Mode::Insert {
            let selection = self.editor.cursor.edit_selection(&self.buffer);
            let (selection, _) =
                self.edit(ctx, &selection, c, None, true, EditType::InsertChars);
            let editor = Arc::make_mut(&mut self.editor);
            editor.cursor.mode = CursorMode::Insert(selection);
            editor.cursor.horiz = None;
            self.update_completion(ctx);
        }
    }
}

fn process_get_references(
    offset: usize,
    result: Result<Value, xi_rpc::Error>,
    event_sink: ExtEventSink,
) -> Result<()> {
    let res = result.map_err(|e| anyhow!("{:?}", e))?;
    let locations: Vec<Location> = serde_json::from_value(res)?;
    if locations.len() == 0 {
        return Ok(());
    }
    if locations.len() == 1 {
        let location = &locations[0];
        event_sink.submit_command(
            LAPCE_UI_COMMAND,
            LapceUICommand::GotoReference(
                offset,
                EditorLocationNew {
                    path: PathBuf::from(location.uri.path()),
                    position: location.range.start.clone(),
                    scroll_offset: None,
                },
            ),
            Target::Auto,
        );
    }
    event_sink.submit_command(
        LAPCE_UI_COMMAND,
        LapceUICommand::PaletteReferences(offset, locations),
        Target::Auto,
    );
    Ok(())
}

pub fn hex_to_color(hex: &str) -> Result<Color> {
    let hex = hex.trim_start_matches("#");
    let (r, g, b, a) = match hex.len() {
        3 => (
            format!("{}{}", &hex[0..0], &hex[0..0]),
            format!("{}{}", &hex[1..1], &hex[1..1]),
            format!("{}{}", &hex[2..2], &hex[2..2]),
            "ff".to_string(),
        ),
        6 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            "ff".to_string(),
        ),
        8 => (
            hex[0..2].to_string(),
            hex[2..4].to_string(),
            hex[4..6].to_string(),
            hex[6..8].to_string(),
        ),
        _ => return Err(anyhow!("invalid hex color")),
    };
    Ok(Color::rgba8(
        u8::from_str_radix(&r, 16)?,
        u8::from_str_radix(&g, 16)?,
        u8::from_str_radix(&b, 16)?,
        u8::from_str_radix(&a, 16)?,
    ))
}
