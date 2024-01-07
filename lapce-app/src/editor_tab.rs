use std::{
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use floem::{
    peniko::{
        kurbo::{Point, Rect},
        Color,
    },
    reactive::{create_memo, create_rw_signal, Memo, ReadSignal, RwSignal, Scope},
};
use lapce_rpc::plugin::VoltID;
use serde::{Deserialize, Serialize};

use crate::{
    config::{color::LapceColor, icon::LapceIcons, LapceConfig},
    doc::{DocContent, Document},
    editor::{
        diff::{DiffEditorData, DiffEditorInfo},
        location::EditorLocation,
        EditorData, EditorInfo,
    },
    id::{
        DiffEditorId, EditorId, EditorTabId, KeymapId, SettingsId, SplitId,
        ThemeColorSettingsId, VoltViewId,
    },
    main_split::MainSplitData,
    plugin::PluginData,
    window_tab::WindowTabData,
};

#[derive(Clone, Serialize, Deserialize)]
pub enum EditorTabChildInfo {
    Editor(EditorInfo),
    DiffEditor(DiffEditorInfo),
    Settings,
    ThemeColorSettings,
    Keymap,
    Volt(VoltID),
}

impl EditorTabChildInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        editor_tab_id: EditorTabId,
    ) -> EditorTabChild {
        match &self {
            EditorTabChildInfo::Editor(editor_info) => {
                let editor_data = editor_info.to_data(data, editor_tab_id);
                EditorTabChild::Editor(editor_data.editor_id)
            }
            EditorTabChildInfo::DiffEditor(diff_editor_info) => {
                let diff_editor_data = diff_editor_info.to_data(data, editor_tab_id);
                EditorTabChild::DiffEditor(diff_editor_data.id)
            }
            EditorTabChildInfo::Settings => {
                EditorTabChild::Settings(SettingsId::next())
            }
            EditorTabChildInfo::ThemeColorSettings => {
                EditorTabChild::ThemeColorSettings(ThemeColorSettingsId::next())
            }
            EditorTabChildInfo::Keymap => EditorTabChild::Keymap(KeymapId::next()),
            EditorTabChildInfo::Volt(id) => {
                EditorTabChild::Volt(VoltViewId::next(), id.to_owned())
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct EditorTabInfo {
    pub active: usize,
    pub is_focus: bool,
    pub children: Vec<EditorTabChildInfo>,
}

impl EditorTabInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        split: SplitId,
    ) -> RwSignal<EditorTabData> {
        let editor_tab_id = EditorTabId::next();
        let editor_tab_data = {
            let cx = data.scope.create_child();
            let editor_tab_data = EditorTabData {
                scope: cx,
                editor_tab_id,
                split,
                active: self.active,
                children: self
                    .children
                    .iter()
                    .map(|child| {
                        (
                            cx.create_rw_signal(0),
                            cx.create_rw_signal(Rect::ZERO),
                            child.to_data(data.clone(), editor_tab_id),
                        )
                    })
                    .collect(),
                layout_rect: Rect::ZERO,
                window_origin: Point::ZERO,
                locations: cx.create_rw_signal(im::Vector::new()),
                current_location: cx.create_rw_signal(0),
            };
            cx.create_rw_signal(editor_tab_data)
        };
        if self.is_focus {
            data.active_editor_tab.set(Some(editor_tab_id));
        }
        data.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab_data);
        });
        editor_tab_data
    }
}

pub enum EditorTabChildSource {
    Editor {
        path: PathBuf,
        doc: Rc<Document>,
    },
    DiffEditor {
        left: Rc<Document>,
        right: Rc<Document>,
    },
    NewFileEditor,
    Settings,
    ThemeColorSettings,
    Keymap,
    Volt(VoltID),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorTabChild {
    Editor(EditorId),
    DiffEditor(DiffEditorId),
    Settings(SettingsId),
    ThemeColorSettings(ThemeColorSettingsId),
    Keymap(KeymapId),
    Volt(VoltViewId, VoltID),
}

#[derive(PartialEq)]
pub struct EditorTabChildViewInfo {
    pub icon: String,
    pub color: Option<Color>,
    pub path: String,
    pub confirmed: Option<RwSignal<bool>>,
    pub is_pristine: bool,
}

impl EditorTabChild {
    pub fn id(&self) -> u64 {
        match self {
            EditorTabChild::Editor(id) => id.to_raw(),
            EditorTabChild::DiffEditor(id) => id.to_raw(),
            EditorTabChild::Settings(id) => id.to_raw(),
            EditorTabChild::ThemeColorSettings(id) => id.to_raw(),
            EditorTabChild::Keymap(id) => id.to_raw(),
            EditorTabChild::Volt(id, _) => id.to_raw(),
        }
    }

    pub fn is_settings(&self) -> bool {
        matches!(self, EditorTabChild::Settings(_))
    }

    pub fn child_info(&self, data: &WindowTabData) -> EditorTabChildInfo {
        match &self {
            EditorTabChild::Editor(editor_id) => {
                let editor_data = data
                    .main_split
                    .editors
                    .get_untracked()
                    .get(editor_id)
                    .cloned()
                    .unwrap();
                EditorTabChildInfo::Editor(editor_data.editor_info(data))
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor_data = data
                    .main_split
                    .diff_editors
                    .get_untracked()
                    .get(diff_editor_id)
                    .cloned()
                    .unwrap();
                EditorTabChildInfo::DiffEditor(diff_editor_data.diff_editor_info())
            }
            EditorTabChild::Settings(_) => EditorTabChildInfo::Settings,
            EditorTabChild::ThemeColorSettings(_) => {
                EditorTabChildInfo::ThemeColorSettings
            }
            EditorTabChild::Keymap(_) => EditorTabChildInfo::Keymap,
            EditorTabChild::Volt(_, id) => EditorTabChildInfo::Volt(id.to_owned()),
        }
    }

    pub fn view_info(
        &self,
        editors: RwSignal<im::HashMap<EditorId, Rc<EditorData>>>,
        diff_editors: RwSignal<im::HashMap<DiffEditorId, DiffEditorData>>,
        plugin: PluginData,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Memo<EditorTabChildViewInfo> {
        match self.clone() {
            EditorTabChild::Editor(editor_id) => create_memo(move |_| {
                let config = config.get();
                let editor_data =
                    editors.with(|editors| editors.get(&editor_id).cloned());
                let path = if let Some(editor_data) = editor_data {
                    let doc = editor_data.view.doc.get();
                    let (content, is_pristine, confirmed) = (
                        doc.content.get(),
                        doc.buffer.with(|b| b.is_pristine()),
                        editor_data.confirmed,
                    );
                    match content {
                        DocContent::File { path, .. } => {
                            Some((path, confirmed, is_pristine))
                        }
                        DocContent::Local => None,
                        DocContent::History(_) => None,
                        DocContent::Scratch { name, .. } => {
                            Some((PathBuf::from(name), confirmed, is_pristine))
                        }
                    }
                } else {
                    None
                };
                let (icon, color, path, confirmed, is_pristine) = match path {
                    Some((path, confirmed, is_pritine)) => {
                        let (svg, color) = config.file_svg(&path);
                        (
                            svg,
                            color,
                            path.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                            confirmed,
                            is_pritine,
                        )
                    }
                    None => (
                        config.ui_svg(LapceIcons::FILE),
                        Some(config.color(LapceColor::LAPCE_ICON_ACTIVE)),
                        "local".to_string(),
                        create_rw_signal(true),
                        true,
                    ),
                };
                EditorTabChildViewInfo {
                    icon,
                    color,
                    path,
                    confirmed: Some(confirmed),
                    is_pristine,
                }
            }),
            EditorTabChild::DiffEditor(diff_editor_id) => create_memo(move |_| {
                let config = config.get();
                let diff_editor_data = diff_editors
                    .with(|diff_editors| diff_editors.get(&diff_editor_id).cloned());
                let confirmed = diff_editor_data.as_ref().map(|d| d.confirmed);

                let info = diff_editor_data
                    .map(|diff_editor_data| {
                        [diff_editor_data.left, diff_editor_data.right].map(|data| {
                            let (content, is_pristine) = data.view.doc.with(|doc| {
                                (
                                    doc.content.get(),
                                    doc.buffer.with(|b| b.is_pristine()),
                                )
                            });
                            match content {
                                DocContent::File { path, .. } => {
                                    Some((path, is_pristine))
                                }
                                DocContent::Local => None,
                                DocContent::History(_) => None,
                                DocContent::Scratch { name, .. } => {
                                    Some((PathBuf::from(name), is_pristine))
                                }
                            }
                        })
                    })
                    .unwrap_or([None, None]);

                let (icon, color, path, is_pristine) = match info {
                    [Some((path, is_pristine)), None]
                    | [None, Some((path, is_pristine))] => {
                        let (svg, color) = config.file_svg(&path);
                        (
                            svg,
                            color,
                            format!(
                                "{} (Diff)",
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            ),
                            is_pristine,
                        )
                    }
                    [Some((left_path, left_is_pristine)), Some((right_path, right_is_pristine))] =>
                    {
                        let (svg, color) =
                            config.files_svg(&[&left_path, &right_path]);
                        let [left_file_name, right_file_name] =
                            [&left_path, &right_path].map(|path| {
                                path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            });
                        (
                            svg,
                            color,
                            format!("{left_file_name} - {right_file_name} (Diff)"),
                            left_is_pristine && right_is_pristine,
                        )
                    }
                    [None, None] => (
                        config.ui_svg(LapceIcons::FILE),
                        Some(config.color(LapceColor::LAPCE_ICON_ACTIVE)),
                        "local".to_string(),
                        true,
                    ),
                };
                EditorTabChildViewInfo {
                    icon,
                    color,
                    path,
                    confirmed,
                    is_pristine,
                }
            }),
            EditorTabChild::Settings(_) => create_memo(move |_| {
                let config = config.get();
                EditorTabChildViewInfo {
                    icon: config.ui_svg(LapceIcons::SETTINGS),
                    color: Some(config.color(LapceColor::LAPCE_ICON_ACTIVE)),
                    path: "Settings".to_string(),
                    confirmed: None,
                    is_pristine: true,
                }
            }),
            EditorTabChild::ThemeColorSettings(_) => create_memo(move |_| {
                let config = config.get();
                EditorTabChildViewInfo {
                    icon: config.ui_svg(LapceIcons::SYMBOL_COLOR),
                    color: Some(config.color(LapceColor::LAPCE_ICON_ACTIVE)),
                    path: "Theme Colors".to_string(),
                    confirmed: None,
                    is_pristine: true,
                }
            }),
            EditorTabChild::Keymap(_) => create_memo(move |_| {
                let config = config.get();
                EditorTabChildViewInfo {
                    icon: config.ui_svg(LapceIcons::KEYBOARD),
                    color: Some(config.color(LapceColor::LAPCE_ICON_ACTIVE)),
                    path: "Keyboard Shortcuts".to_string(),
                    confirmed: None,
                    is_pristine: true,
                }
            }),
            EditorTabChild::Volt(_, id) => create_memo(move |_| {
                let config = config.get();
                let display_name = plugin
                    .installed
                    .with(|volts| volts.get(&id).cloned())
                    .map(|volt| volt.meta.with(|m| m.display_name.clone()))
                    .or_else(|| {
                        plugin.available.volts.with(|volts| {
                            let volt = volts.get(&id);
                            volt.map(|volt| {
                                volt.info.with(|m| m.display_name.clone())
                            })
                        })
                    })
                    .unwrap_or_else(|| id.name.clone());
                EditorTabChildViewInfo {
                    icon: config.ui_svg(LapceIcons::EXTENSIONS),
                    color: Some(config.color(LapceColor::LAPCE_ICON_ACTIVE)),
                    path: display_name,
                    confirmed: None,
                    is_pristine: true,
                }
            }),
        }
    }
}

#[derive(Clone)]
pub struct EditorTabData {
    pub scope: Scope,
    pub split: SplitId,
    pub editor_tab_id: EditorTabId,
    pub active: usize,
    pub children: Vec<(RwSignal<usize>, RwSignal<Rect>, EditorTabChild)>,
    pub window_origin: Point,
    pub layout_rect: Rect,
    pub locations: RwSignal<im::Vector<EditorLocation>>,
    pub current_location: RwSignal<usize>,
}

impl EditorTabData {
    pub fn get_editor(
        &self,
        editors: &im::HashMap<EditorId, Rc<EditorData>>,
        path: &Path,
    ) -> Option<(usize, Rc<EditorData>)> {
        for (i, child) in self.children.iter().enumerate() {
            if let (_, _, EditorTabChild::Editor(editor_id)) = child {
                if let Some(editor) = editors.get(editor_id) {
                    let is_path =
                        editor.view.doc.get_untracked().content.with_untracked(
                            |content| {
                                if let DocContent::File { path: p, .. } = content {
                                    p == path
                                } else {
                                    false
                                }
                            },
                        );
                    if is_path {
                        return Some((i, editor.clone()));
                    }
                }
            }
        }
        None
    }

    pub fn get_unconfirmed_editor_tab_child(
        &self,
        editors: &im::HashMap<EditorId, Rc<EditorData>>,
        diff_editors: &im::HashMap<EditorId, DiffEditorData>,
    ) -> Option<(usize, EditorTabChild)> {
        for (i, (_, _, child)) in self.children.iter().enumerate() {
            match child {
                EditorTabChild::Editor(editor_id) => {
                    if let Some(editor) = editors.get(editor_id) {
                        let confirmed = editor.confirmed.get_untracked();
                        if !confirmed {
                            return Some((i, child.clone()));
                        }
                    }
                }
                EditorTabChild::DiffEditor(diff_editor_id) => {
                    if let Some(diff_editor) = diff_editors.get(diff_editor_id) {
                        let confirmed = diff_editor.confirmed.get_untracked();
                        if !confirmed {
                            return Some((i, child.clone()));
                        }
                    }
                }
                _ => (),
            }
        }
        None
    }

    pub fn tab_info(&self, data: &WindowTabData) -> EditorTabInfo {
        let info = EditorTabInfo {
            active: self.active,
            is_focus: data.main_split.active_editor_tab.get_untracked()
                == Some(self.editor_tab_id),
            children: self
                .children
                .iter()
                .map(|(_, _, child)| child.child_info(data))
                .collect(),
        };
        info
    }
}
