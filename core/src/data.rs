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
    theme, Color, Data, Env, FontDescriptor, FontFamily, Lens, WidgetId, WindowId,
};
use im;
use parking_lot::Mutex;
use xi_rpc::{RpcLoop, RpcPeer};

use crate::{
    buffer::{Buffer, BufferId, BufferNew, BufferState},
    proxy::{LapceProxy, ProxyHandlerNew},
    state::{LapceWorkspace, LapceWorkspaceType},
    theme::LapceTheme,
};

#[derive(Clone, Data)]
pub struct LapceData {
    pub windows: im::HashMap<WindowId, LapceWindowData>,
    pub theme: im::HashMap<String, Color>,
    pub theme_changed: bool,
}

impl LapceData {
    pub fn load() -> Self {
        let mut windows = im::HashMap::new();
        let window = LapceWindowData::new();
        windows.insert(WindowId::next(), window);
        Self {
            windows,
            theme: Self::get_theme().unwrap_or(im::HashMap::new()),
            theme_changed: true,
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
}

impl Data for LapceWindowData {
    fn same(&self, other: &Self) -> bool {
        self.active == other.active && self.tabs.same(&other.tabs)
    }
}

impl LapceWindowData {
    pub fn new() -> Self {
        let mut tabs = im::HashMap::new();
        let tab_id = WidgetId::next();
        let tab = LapceTabData::new(tab_id);
        tabs.insert(tab_id, tab);
        Self {
            tabs,
            active: tab_id,
        }
    }
}

#[derive(Clone, Lens)]
pub struct LapceTabData {
    pub id: WidgetId,
    pub main_split: LapceMainSplitData,
    pub proxy: Arc<LapceProxy>,
}

impl Data for LapceTabData {
    fn same(&self, other: &Self) -> bool {
        self.main_split.same(&other.main_split)
    }
}

impl LapceTabData {
    pub fn new(tab_id: WidgetId) -> Self {
        let proxy = Arc::new(LapceProxy::new(tab_id));
        let main_split = LapceMainSplitData::new(proxy.clone());
        let workspace = LapceWorkspace {
            kind: LapceWorkspaceType::Local,
            path: PathBuf::from("/Users/Lulu/lapce"),
        };
        proxy.start(workspace);
        Self {
            id: tab_id,
            main_split,
            proxy,
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
        let mut tab = data.tabs.get_mut(&self.0).unwrap();
        f(&mut tab)
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
        let mut tab = data.windows.get_mut(&self.0).unwrap();
        f(&mut tab)
    }
}

#[derive(Clone, Data, Lens)]
pub struct LapceMainSplitData {
    pub editors: im::HashMap<WidgetId, Arc<LapceEditorData>>,
    pub buffers: im::HashMap<BufferId, BufferState>,
    pub open_files: im::HashMap<PathBuf, BufferId>,
    pub proxy: Arc<LapceProxy>,
}

impl LapceMainSplitData {
    pub fn new(proxy: Arc<LapceProxy>) -> Self {
        let mut editors = im::HashMap::new();
        let editor = LapceEditorData {
            buffer: Some(PathBuf::from("/Users/Lulu/lapce/Cargo.toml")),
        };
        editors.insert(WidgetId::next(), Arc::new(editor));
        let buffers = im::HashMap::new();
        let open_files = im::HashMap::new();
        Self {
            editors,
            buffers,
            proxy,
            open_files,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LapceEditorData {
    pub buffer: Option<PathBuf>,
}

#[derive(Clone, Data, Lens, Debug)]
pub struct LapceEditorViewData {
    pub editor: Arc<LapceEditorData>,
    pub buffer: Option<BufferState>,
}

pub struct LapceEditorLens(pub WidgetId);

impl Lens<LapceMainSplitData, LapceEditorViewData> for LapceEditorLens {
    fn with<V, F: FnOnce(&LapceEditorViewData) -> V>(
        &self,
        data: &LapceMainSplitData,
        f: F,
    ) -> V {
        let editor = data.editors.get(&self.0).unwrap();
        let editor_view = LapceEditorViewData {
            buffer: editor
                .buffer
                .as_ref()
                .map(|b| {
                    data.open_files
                        .get(b)
                        .map(|id| data.buffers.get(id).map(|b| b.clone()))
                })
                .flatten()
                .flatten(),
            editor: editor.clone(),
        };
        f(&editor_view)
    }

    fn with_mut<V, F: FnOnce(&mut LapceEditorViewData) -> V>(
        &self,
        data: &mut LapceMainSplitData,
        f: F,
    ) -> V {
        let editor = data.editors.get(&self.0).unwrap();
        let mut editor_view = LapceEditorViewData {
            buffer: editor
                .buffer
                .as_ref()
                .map(|b| {
                    data.open_files
                        .get(b)
                        .map(|id| data.buffers.get(id).map(|b| b.clone()))
                })
                .flatten()
                .flatten(),
            editor: editor.clone(),
        };
        f(&mut editor_view)
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
