use std::{
    fs::File,
    io::{BufReader, Read},
    path::PathBuf,
    process::{self, Stdio},
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_utils::sync::WaitGroup;
use druid::{
    theme, Color, Command, Data, Env, EventCtx, FontDescriptor, FontFamily,
    KeyEvent, Lens, Rect, Size, Target, Vec2, WidgetId, WindowId,
};
use im;
use parking_lot::Mutex;
use xi_core_lib::selection::InsertDrift;
use xi_rope::{DeltaBuilder, RopeDelta};
use xi_rpc::{RpcLoop, RpcPeer};

use crate::{
    buffer::{
        previous_has_unmatched_pair, Buffer, BufferId, BufferNew, BufferState,
    },
    command::{LapceCommand, LapceUICommand, LAPCE_UI_COMMAND},
    keypress::{KeyPressData, KeyPressFocus},
    movement::{Cursor, CursorMode, LinePosition, Movement, SelRegion, Selection},
    proxy::{LapceProxy, ProxyHandlerNew},
    split::SplitMoveDirection,
    state::{LapceWorkspace, LapceWorkspaceType, Mode, VisualMode},
    theme::LapceTheme,
};

#[derive(Clone, Data)]
pub struct LapceData {
    pub windows: im::HashMap<WindowId, LapceWindowData>,
    pub theme: im::HashMap<String, Color>,
    pub theme_changed: bool,
    pub keypress: Arc<KeyPressData>,
}

impl LapceData {
    pub fn load() -> Self {
        let mut windows = im::HashMap::new();
        let keypress = Arc::new(KeyPressData::new());
        let window = LapceWindowData::new(keypress.clone());
        windows.insert(WindowId::next(), window);
        Self {
            windows,
            theme: Self::get_theme().unwrap_or(im::HashMap::new()),
            theme_changed: true,
            keypress,
        }
    }

    fn get_theme() -> Result<im::HashMap<String, Color>> {
        let mut f = File::open("/Users/Lulu/lapce/.lapce/theme.toml")?;
        let mut content = vec![];
        f.read_to_end(&mut content)?;
        let toml_theme: im::HashMap<String, String> = toml::from_slice(&content)?;

        let mut theme = im::HashMap::new();
        for (name, hex) in toml_theme.iter() {
            if let Ok(color) = hex_to_color(hex) {
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
        env.set(theme::SCROLLBAR_COLOR, hex_to_color("#c4c4c4").unwrap());
    }
}

#[derive(Clone)]
pub struct LapceWindowData {
    pub tabs: im::HashMap<WidgetId, LapceTabData>,
    pub active: WidgetId,
    pub keypress: Arc<KeyPressData>,
}

impl Data for LapceWindowData {
    fn same(&self, other: &Self) -> bool {
        self.active == other.active && self.tabs.same(&other.tabs)
    }
}

impl LapceWindowData {
    pub fn new(keypress: Arc<KeyPressData>) -> Self {
        let mut tabs = im::HashMap::new();
        let tab_id = WidgetId::next();
        let tab = LapceTabData::new(tab_id, keypress.clone());
        tabs.insert(tab_id, tab);
        Self {
            tabs,
            active: tab_id,
            keypress,
        }
    }
}

#[derive(Clone, Lens)]
pub struct LapceTabData {
    pub id: WidgetId,
    pub main_split: LapceMainSplitData,
    pub proxy: Arc<LapceProxy>,
    pub keypress: Arc<KeyPressData>,
}

impl Data for LapceTabData {
    fn same(&self, other: &Self) -> bool {
        self.main_split.same(&other.main_split)
    }
}

impl LapceTabData {
    pub fn new(tab_id: WidgetId, keypress: Arc<KeyPressData>) -> Self {
        let proxy = Arc::new(LapceProxy::new(tab_id));
        let main_split = LapceMainSplitData::new();
        let workspace = LapceWorkspace {
            kind: LapceWorkspaceType::Local,
            path: PathBuf::from("/Users/Lulu/lapce"),
        };
        proxy.start(workspace);
        Self {
            id: tab_id,
            main_split,
            proxy,
            keypress,
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
        let result = f(&mut tab);
        data.keypress = tab.keypress.clone();
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
        let result = f(&mut win);
        data.keypress = win.keypress.clone();
        if !win.same(data.windows.get(&self.0).unwrap()) {
            data.windows.insert(self.0, win);
        }
        result
    }
}

#[derive(Clone, Data, Lens, Debug)]
pub struct LapceMainSplitData {
    pub split_id: Arc<WidgetId>,
    pub focus: Arc<WidgetId>,
    pub editors: im::HashMap<WidgetId, Arc<LapceEditorData>>,
    pub buffers: im::HashMap<BufferId, Arc<BufferNew>>,
    pub open_files: im::HashMap<PathBuf, BufferId>,
}

impl LapceMainSplitData {
    pub fn notify_update_text_layouts(
        &self,
        ctx: &mut EventCtx,
        buffer_id: &BufferId,
    ) {
        for (editor_id, editor) in &self.editors {
            let editor_buffer_id = self.open_files.get(&editor.buffer).unwrap();
            if editor_buffer_id == buffer_id {
                ctx.submit_command(Command::new(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FillTextLayouts,
                    Target::Widget(*editor_id),
                ));
            }
        }
    }
}

impl LapceMainSplitData {
    pub fn new() -> Self {
        let split_id = Arc::new(WidgetId::next());
        let mut editors = im::HashMap::new();
        let path = PathBuf::from("/Users/Lulu/lapce/src/editor_old.rs");
        let editor = LapceEditorData::new(*split_id, path.clone());
        let editor_id = editor.editor_id;
        editors.insert(editor.view_id, Arc::new(editor));
        let buffer = BufferNew::new(path.clone());
        let mut open_files = im::HashMap::new();
        open_files.insert(path.clone(), buffer.id);
        let mut buffers = im::HashMap::new();
        buffers.insert(buffer.id, Arc::new(buffer));
        Self {
            split_id,
            editors,
            buffers,
            open_files,
            focus: Arc::new(editor_id),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LapceEditorData {
    pub split_id: WidgetId,
    pub view_id: WidgetId,
    pub editor_id: WidgetId,
    pub buffer: PathBuf,
    pub scroll_offset: Vec2,
    pub cursor: Cursor,
    pub size: Size,
}

impl LapceEditorData {
    pub fn new(split_id: WidgetId, buffer: PathBuf) -> Self {
        Self {
            split_id,
            view_id: WidgetId::next(),
            editor_id: WidgetId::next(),
            buffer,
            scroll_offset: Vec2::ZERO,
            cursor: Cursor::default(),
            size: Size::ZERO,
        }
    }
}

#[derive(Clone, Data, Lens, Debug)]
pub struct LapceEditorViewData {
    pub main_split: LapceMainSplitData,
    pub editor: Arc<LapceEditorData>,
    pub buffer: Arc<BufferNew>,
    pub keypress: Arc<KeyPressData>,
}

impl LapceEditorViewData {
    pub fn key_down(&mut self, ctx: &mut EventCtx, key_event: &KeyEvent) {
        let mut keypress = self.keypress.clone();
        let k = Arc::make_mut(&mut keypress);
        k.key_down(ctx, key_event, self);
        self.keypress = keypress;
    }

    pub fn buffer_mut(&mut self) -> &mut BufferNew {
        Arc::make_mut(&mut self.buffer)
    }

    pub fn fill_text_layouts(&mut self, ctx: &mut EventCtx, env: &Env) {
        let line_height = env.get(LapceTheme::EDITOR_LINE_HEIGHT);
        let start_line = (self.editor.scroll_offset.y / line_height) as usize;
        let size = ctx.size();
        let num_lines = (size.height / line_height) as usize;
        let text = ctx.text();
        let buffer = self.buffer_mut();
        for line in start_line..start_line + num_lines + 1 {
            buffer.update_line_layouts(text, line, env);
        }
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

    pub fn do_move(&mut self, movement: &Movement, count: usize) {
        match &self.editor.cursor.mode {
            &CursorMode::Normal(offset) => {
                let (new_offset, horiz) = self.buffer.move_offset(
                    offset,
                    self.editor.cursor.horiz.as_ref(),
                    count,
                    movement,
                    false,
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
                    true,
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
                let mut new_selection = Selection::new();
                for region in selection.regions() {
                    let (offset, horiz) = self.buffer.move_offset(
                        region.end(),
                        region.horiz(),
                        count,
                        movement,
                        true,
                    );
                    let new_region =
                        SelRegion::new(offset, offset, Some(horiz.clone()));
                    new_selection.add_region(new_region);
                }
                let editor = Arc::make_mut(&mut self.editor);
                editor.cursor.mode = CursorMode::Insert(new_selection);
                editor.cursor.horiz = None;
            }
        }
    }

    pub fn cusor_region(&self, env: &Env) -> Rect {
        self.editor.cursor.region(&self.buffer, env)
    }

    pub fn insert_new_line(&mut self, ctx: &mut EventCtx, offset: usize) {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let line_content = self.buffer.line_content(line);
        let line_indent = self.buffer.indent_on_line(line);

        let indent = if previous_has_unmatched_pair(&line_content, col) {
            format!("{}    ", line_indent)
        } else if line_indent.len() >= col {
            line_indent[..col].to_string()
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

        let selection = self.edit(ctx, &selection, &content);
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor.mode = CursorMode::Insert(selection);
        editor.cursor.horiz = None;
    }

    fn set_cursor(&mut self, cursor: Cursor) {
        let editor = Arc::make_mut(&mut self.editor);
        editor.cursor = cursor;
    }

    fn edit(
        &mut self,
        ctx: &mut EventCtx,
        selection: &Selection,
        c: &str,
    ) -> Selection {
        let buffer = self.buffer_mut();
        let delta = buffer.edit(ctx, &selection, c);
        let buffer_id = buffer.id;
        self.main_split.notify_update_text_layouts(ctx, &buffer_id);
        self.inactive_apply_delta(&delta);
        let selection = selection.apply_delta(&delta, true, InsertDrift::Default);
        selection
    }

    fn inactive_apply_delta(&mut self, delta: &RopeDelta) {
        let open_files = self.main_split.open_files.clone();
        for (view_id, editor) in self.main_split.editors.iter_mut() {
            if view_id != &self.editor.view_id {
                let editor_buffer_id = open_files.get(&editor.buffer).unwrap();
                if editor_buffer_id == &self.buffer.id {
                    Arc::make_mut(editor).cursor.apply_delta(delta);
                }
            }
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
            buffer: main_split
                .buffers
                .get(main_split.open_files.get(&editor.buffer).unwrap())
                .unwrap()
                .clone(),
            editor: editor.clone(),
            main_split: main_split.clone(),
            keypress: data.keypress.clone(),
        };
        f(&editor_view)
    }

    fn with_mut<V, F: FnOnce(&mut LapceEditorViewData) -> V>(
        &self,
        data: &mut LapceTabData,
        f: F,
    ) -> V {
        let editor = data.main_split.editors.get(&self.0).unwrap().clone();
        let buffer_id = *data.main_split.open_files.get(&editor.buffer).unwrap();
        let mut editor_view = LapceEditorViewData {
            buffer: data.main_split.buffers.get(&buffer_id).unwrap().clone(),
            editor: editor.clone(),
            main_split: data.main_split.clone(),
            keypress: data.keypress.clone(),
        };
        let result = f(&mut editor_view);

        data.keypress = editor_view.keypress.clone();
        data.main_split = editor_view.main_split.clone();
        if !editor.same(&editor_view.editor) {
            data.main_split
                .editors
                .insert(self.0, editor_view.editor.clone());
        }
        if !data
            .main_split
            .buffers
            .get(&buffer_id)
            .unwrap()
            .same(&editor_view.buffer)
        {
            data.main_split
                .buffers
                .insert(buffer_id, editor_view.buffer.clone());
        }

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
        match condition.trim() {
            _ => false,
        }
    }

    fn run_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &LapceCommand,
        count: Option<usize>,
    ) {
        if let Some(movement) = self.move_command(count, cmd) {
            self.do_move(&movement, count.unwrap_or(1));
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
            LapceCommand::InsertMode => {
                Arc::make_mut(&mut self.editor).cursor.mode = CursorMode::Insert(
                    Selection::caret(self.editor.cursor.offset()),
                );
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
            LapceCommand::DeleteForewardAndInsert => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                let selection = self.edit(ctx, &selection, "");
                self.set_cursor(Cursor::new(CursorMode::Insert(selection), None));
            }
            LapceCommand::InsertNewLine => {
                let selection = self.editor.cursor.edit_selection(&self.buffer);
                if selection.regions().len() > 1 {
                    let selection = self.edit(ctx, &selection, "\n");
                    self.set_cursor(Cursor::new(
                        CursorMode::Insert(selection),
                        None,
                    ));
                    return;
                };
                self.insert_new_line(ctx, self.editor.cursor.offset());
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
            LapceCommand::NormalMode => {
                let offset = match &self.editor.cursor.mode {
                    CursorMode::Insert(selection) => {
                        self.buffer
                            .move_offset(
                                selection.get_cursor_offset(),
                                None,
                                1,
                                &Movement::Left,
                                false,
                            )
                            .0
                    }
                    CursorMode::Visual { start, end, mode } => {
                        self.buffer.offset_line_end(*end, false)
                    }
                    CursorMode::Normal(offset) => *offset,
                };
                let mut cursor = &mut Arc::make_mut(&mut self.editor).cursor;
                cursor.mode = CursorMode::Normal(offset);
                cursor.horiz = None;
            }
            _ => (),
        }
    }

    fn insert(&mut self, ctx: &mut EventCtx, c: &str) {
        if self.get_mode() == Mode::Insert {
            let selection = self.editor.cursor.edit_selection(&self.buffer);
            let selection = self.edit(ctx, &selection, c);
            let editor = Arc::make_mut(&mut self.editor);
            editor.cursor.mode = CursorMode::Insert(selection);
            editor.cursor.horiz = None;
        }
    }
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
