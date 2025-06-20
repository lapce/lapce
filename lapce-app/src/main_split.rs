use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use floem::{
    action::save_as,
    ext_event::create_ext_action,
    file::{FileDialogOptions, FileInfo},
    keyboard::Modifiers,
    peniko::kurbo::{Point, Rect, Vec2},
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::editor::id::EditorId,
};
use itertools::Itertools;
use lapce_core::{
    buffer::rope_text::RopeText, command::FocusCommand, cursor::Cursor,
    rope_text_pos::RopeTextPosition, selection::Selection, syntax::Syntax,
};
use lapce_rpc::{
    buffer::BufferId,
    core::FileChanged,
    plugin::{PluginId, VoltID},
    proxy::ProxyResponse,
};
use lapce_xi_rope::{Rope, spans::SpansBuilder};
use lsp_types::{
    CodeAction, CodeActionOrCommand, DiagnosticSeverity, DocumentChangeOperation,
    DocumentChanges, OneOf, Position, TextEdit, Url, WorkspaceEdit,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{Level, event};

use crate::{
    alert::AlertButton,
    code_lens::CodeLensData,
    command::InternalCommand,
    doc::{DiagnosticData, Doc, DocContent, DocHistory, EditorDiagnostic},
    editor::{
        EditorData,
        diff::DiffEditorData,
        location::{EditorLocation, EditorPosition},
    },
    editor_tab::{
        EditorTabChild, EditorTabChildSource, EditorTabData, EditorTabInfo,
    },
    id::{
        DiffEditorId, EditorTabId, KeymapId, SettingsId, SplitId,
        ThemeColorSettingsId, VoltViewId,
    },
    keypress::{EventRef, KeyPressData, KeyPressHandle},
    panel::implementation_view::ReferencesRoot,
    window_tab::{CommonData, Focus, WindowTabData},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
pub enum SplitMoveDirection {
    Up,
    Down,
    Right,
    Left,
}

impl SplitMoveDirection {
    pub fn direction(&self) -> SplitDirection {
        match self {
            SplitMoveDirection::Up | SplitMoveDirection::Down => {
                SplitDirection::Horizontal
            }
            SplitMoveDirection::Left | SplitMoveDirection::Right => {
                SplitDirection::Vertical
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitContent {
    EditorTab(EditorTabId),
    Split(SplitId),
}

impl SplitContent {
    pub fn id(&self) -> u64 {
        match self {
            SplitContent::EditorTab(id) => id.to_raw(),
            SplitContent::Split(id) => id.to_raw(),
        }
    }

    pub fn content_info(&self, data: &WindowTabData) -> SplitContentInfo {
        match &self {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab_data = data
                    .main_split
                    .editor_tabs
                    .get_untracked()
                    .get(editor_tab_id)
                    .cloned()
                    .unwrap();
                SplitContentInfo::EditorTab(
                    editor_tab_data.get_untracked().tab_info(data),
                )
            }
            SplitContent::Split(split_id) => {
                let split_data = data
                    .main_split
                    .splits
                    .get_untracked()
                    .get(split_id)
                    .cloned()
                    .unwrap();
                SplitContentInfo::Split(split_data.get_untracked().split_info(data))
            }
        }
    }
}

#[derive(Clone)]
pub struct SplitData {
    pub scope: Scope,
    pub parent_split: Option<SplitId>,
    pub split_id: SplitId,
    pub children: Vec<(RwSignal<f64>, SplitContent)>,
    pub direction: SplitDirection,
    pub window_origin: Point,
    pub layout_rect: Rect,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SplitInfo {
    pub children: Vec<SplitContentInfo>,
    pub direction: SplitDirection,
}

impl SplitInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        parent_split: Option<SplitId>,
        split_id: SplitId,
    ) -> RwSignal<SplitData> {
        let split_data = {
            let cx = data.scope.create_child();
            let split_data = SplitData {
                scope: cx,
                split_id,
                direction: self.direction,
                parent_split,
                children: self
                    .children
                    .iter()
                    .map(|child| {
                        (
                            cx.create_rw_signal(1.0),
                            child.to_data(data.clone(), split_id),
                        )
                    })
                    .collect(),
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
            };
            cx.create_rw_signal(split_data)
        };
        data.splits.update(|splits| {
            splits.insert(split_id, split_data);
        });
        split_data
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum SplitContentInfo {
    EditorTab(EditorTabInfo),
    Split(SplitInfo),
}

impl SplitContentInfo {
    pub fn to_data(
        &self,
        data: MainSplitData,
        parent_split: SplitId,
    ) -> SplitContent {
        match &self {
            SplitContentInfo::EditorTab(tab_info) => {
                let tab_data = tab_info.to_data(data, parent_split);
                SplitContent::EditorTab(
                    tab_data.with_untracked(|tab_data| tab_data.editor_tab_id),
                )
            }
            SplitContentInfo::Split(split_info) => {
                let split_id = SplitId::next();
                split_info.to_data(data, Some(parent_split), split_id);
                SplitContent::Split(split_id)
            }
        }
    }
}

impl SplitData {
    pub fn split_info(&self, data: &WindowTabData) -> SplitInfo {
        SplitInfo {
            direction: self.direction,
            children: self
                .children
                .iter()
                .map(|(_, child)| child.content_info(data))
                .collect(),
        }
    }

    pub fn editor_tab_index(&self, editor_tab_id: EditorTabId) -> Option<usize> {
        self.children
            .iter()
            .position(|(_, c)| c == &SplitContent::EditorTab(editor_tab_id))
    }

    pub fn content_index(&self, content: &SplitContent) -> Option<usize> {
        self.children.iter().position(|(_, c)| c == content)
    }
}

/// All the editors in a main split
#[derive(Clone, Copy)]
pub struct Editors(pub RwSignal<im::HashMap<EditorId, EditorData>>);
impl Editors {
    fn new(cx: Scope) -> Self {
        Self(cx.create_rw_signal(im::HashMap::new()))
    }

    /// Add an editor to the editors.
    /// Returns the id of the editor.
    pub fn insert(&self, editor: EditorData) -> EditorId {
        let id = editor.id();
        self.0.update(|editors| {
            if editors.insert(id, editor).is_some() {
                event!(Level::WARN, "Inserted EditorId that already exists");
            }
        });

        id
    }

    pub fn insert_with_id(&self, id: EditorId, editor: EditorData) {
        self.0.update(|editors| {
            editors.insert(id, editor);
        });
    }

    pub fn new_local(&self, cx: Scope, common: Rc<CommonData>) -> EditorId {
        let editor = EditorData::new_local(cx, *self, common);

        self.insert(editor)
    }

    /// Equivalent to [`Self::new_local`], but immediately gets the created editor.
    pub fn make_local(&self, cx: Scope, common: Rc<CommonData>) -> EditorData {
        let id = self.new_local(cx, common);
        self.editor_untracked(id).unwrap()
    }

    pub fn new_from_doc(
        &self,
        cx: Scope,
        doc: Rc<Doc>,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        confirmed: Option<RwSignal<bool>>,
        common: Rc<CommonData>,
    ) -> EditorId {
        let editor = EditorData::new_doc(
            cx,
            doc,
            editor_tab_id,
            diff_editor_id,
            confirmed,
            common,
        );

        self.insert(editor)
    }

    /// Equivalent to [`Self::new_editor_doc`], but immediately gets the created editor.
    pub fn make_from_doc(
        &self,
        cx: Scope,
        doc: Rc<Doc>,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        confirmed: Option<RwSignal<bool>>,
        common: Rc<CommonData>,
    ) -> EditorData {
        let id = self.new_from_doc(
            cx,
            doc,
            editor_tab_id,
            diff_editor_id,
            confirmed,
            common,
        );
        self.editor_untracked(id).unwrap()
    }

    /// Copy an existing editor which is inserted into [`Editors`]
    pub fn copy(
        &self,
        editor_id: EditorId,
        cx: Scope,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        confirmed: Option<RwSignal<bool>>,
    ) -> Option<EditorId> {
        let editor = self.editor_untracked(editor_id)?;
        let new_editor = editor.copy(cx, editor_tab_id, diff_editor_id, confirmed);

        Some(self.insert(new_editor))
    }

    pub fn make_copy(
        &self,
        editor_id: EditorId,
        cx: Scope,
        editor_tab_id: Option<EditorTabId>,
        diff_editor_id: Option<(EditorTabId, DiffEditorId)>,
        confirmed: Option<RwSignal<bool>>,
    ) -> Option<EditorData> {
        let editor_id =
            self.copy(editor_id, cx, editor_tab_id, diff_editor_id, confirmed)?;
        self.editor_untracked(editor_id)
    }

    pub fn remove(&self, id: EditorId) -> Option<EditorData> {
        self.0.try_update(|editors| editors.remove(&id)).unwrap()
    }

    pub fn get_editor_id_by_path(&self, path: &Path) -> Option<EditorId> {
        self.0.with_untracked(|x| {
            for (id, data) in x {
                if data.doc().content.with_untracked(|x| {
                    if let Some(doc_path) = x.path() {
                        doc_path == path
                    } else {
                        false
                    }
                }) {
                    return Some(*id);
                }
            }
            None
        })
    }

    pub fn contains_untracked(&self, id: EditorId) -> bool {
        self.0.with_untracked(|editors| editors.contains_key(&id))
    }

    /// Get the editor (tracking the signal)
    pub fn editor(&self, id: EditorId) -> Option<EditorData> {
        self.0.with(|editors| editors.get(&id).cloned())
    }

    /// Get the editor (not tracking the signal)
    pub fn editor_untracked(&self, id: EditorId) -> Option<EditorData> {
        self.0.with_untracked(|editors| editors.get(&id).cloned())
    }

    pub fn with_editors<O>(
        &self,
        f: impl FnOnce(&im::HashMap<EditorId, EditorData>) -> O,
    ) -> O {
        self.0.with(f)
    }

    pub fn with_editors_untracked<O>(
        &self,
        f: impl FnOnce(&im::HashMap<EditorId, EditorData>) -> O,
    ) -> O {
        self.0.with_untracked(f)
    }
}

#[derive(Clone)]
pub struct MainSplitData {
    pub scope: Scope,
    pub root_split: SplitId,
    pub active_editor_tab: RwSignal<Option<EditorTabId>>,
    pub splits: RwSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    pub editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    pub editors: Editors,
    pub diff_editors: RwSignal<im::HashMap<DiffEditorId, DiffEditorData>>,
    pub docs: RwSignal<im::HashMap<PathBuf, Rc<Doc>>>,
    pub scratch_docs: RwSignal<im::HashMap<String, Rc<Doc>>>,
    pub diagnostics: RwSignal<im::HashMap<PathBuf, DiagnosticData>>,
    pub references: RwSignal<ReferencesRoot>,
    pub implementations: RwSignal<crate::panel::implementation_view::ReferencesRoot>,
    pub active_editor: Memo<Option<EditorData>>,
    pub find_editor: EditorData,
    pub replace_editor: EditorData,
    pub locations: RwSignal<im::Vector<EditorLocation>>,
    pub current_location: RwSignal<usize>,
    pub width: RwSignal<f64>,
    pub code_lens: RwSignal<CodeLensData>,
    pub common: Rc<CommonData>,
}

impl std::fmt::Debug for MainSplitData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MainSplitData")
            .field("root_split", &self.root_split)
            .finish()
    }
}

impl MainSplitData {
    pub fn new(cx: Scope, common: Rc<CommonData>) -> Self {
        let splits = cx.create_rw_signal(im::HashMap::new());
        let active_editor_tab = cx.create_rw_signal(None);
        let editor_tabs: RwSignal<
            im::HashMap<EditorTabId, RwSignal<EditorTabData>>,
        > = cx.create_rw_signal(im::HashMap::new());
        let editors = Editors::new(cx);
        let diff_editors: RwSignal<im::HashMap<DiffEditorId, DiffEditorData>> =
            cx.create_rw_signal(im::HashMap::new());
        let docs: RwSignal<im::HashMap<PathBuf, Rc<Doc>>> =
            cx.create_rw_signal(im::HashMap::new());
        let scratch_docs = cx.create_rw_signal(im::HashMap::new());
        let locations = cx.create_rw_signal(im::Vector::new());
        let references = cx.create_rw_signal(ReferencesRoot::default());
        let implementations = cx.create_rw_signal(
            crate::panel::implementation_view::ReferencesRoot::default(),
        );
        let current_location = cx.create_rw_signal(0);
        let diagnostics = cx.create_rw_signal(im::HashMap::new());
        let find_editor = editors.make_local(cx, common.clone());
        let replace_editor = editors.make_local(cx, common.clone());

        let active_editor = cx.create_memo(move |_| -> Option<EditorData> {
            let active_editor_tab = active_editor_tab.get()?;
            let editor_tab = editor_tabs
                .with(|editor_tabs| editor_tabs.get(&active_editor_tab).copied())?;
            let (_, _, child) = editor_tab.with(|editor_tab| {
                editor_tab.children.get(editor_tab.active).cloned()
            })?;

            let editor = match child {
                EditorTabChild::Editor(editor_id) => editors.editor(editor_id)?,
                EditorTabChild::DiffEditor(diff_editor_id) => {
                    let diff_editor = diff_editors.with(|diff_editors| {
                        diff_editors.get(&diff_editor_id).cloned()
                    })?;
                    if diff_editor.focus_right.get() {
                        diff_editor.right
                    } else {
                        diff_editor.left
                    }
                }
                _ => return None,
            };

            Some(editor)
        });

        {
            let buffer = find_editor.doc().buffer;
            let find = common.find.clone();
            cx.create_effect(move |_| {
                let content = buffer.with(|buffer| buffer.to_string());
                find.set_find(&content);
            });
        }

        Self {
            scope: cx,
            root_split: SplitId::next(),
            splits,
            active_editor_tab,
            editor_tabs,
            editors,
            diff_editors,
            docs,
            scratch_docs,
            active_editor,
            find_editor,
            replace_editor,
            diagnostics,
            locations,
            current_location,
            width: cx.create_rw_signal(0.0),
            code_lens: cx.create_rw_signal(CodeLensData::new(common.clone())),
            common,
            references,
            implementations,
        }
    }

    pub fn key_down<'a>(
        &self,
        event: impl Into<EventRef<'a>>,
        keypress: &KeyPressData,
    ) -> Option<KeyPressHandle> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let (_, _, child) = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self.editors.editor_untracked(editor_id)?;
                let handle = keypress.key_down(event, &editor);
                editor.get_code_actions();
                Some(handle)
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor =
                    self.diff_editors.with_untracked(|diff_editors| {
                        diff_editors.get(&diff_editor_id).cloned()
                    })?;
                let editor = if diff_editor.focus_right.get_untracked() {
                    &diff_editor.right
                } else {
                    &diff_editor.left
                };
                let handle = keypress.key_down(event, editor);
                editor.get_code_actions();
                Some(handle)
            }
            EditorTabChild::Settings(_) => None,
            EditorTabChild::ThemeColorSettings(_) => None,
            EditorTabChild::Keymap(_) => None,
            EditorTabChild::Volt(_, _) => None,
        }
    }

    fn save_current_jump_location(&self) -> bool {
        if let Some(editor) = self.active_editor.get_untracked() {
            let (cursor, viewport) = (editor.cursor(), editor.viewport());
            let path = editor
                .doc()
                .content
                .with_untracked(|content| content.path().cloned());
            if let Some(path) = path {
                let offset = cursor.with_untracked(|c| c.offset());
                let scroll_offset = viewport.get_untracked().origin().to_vec2();
                return self.save_jump_location(path, offset, scroll_offset);
            }
        }
        false
    }

    pub fn save_jump_location(
        &self,
        path: PathBuf,
        offset: usize,
        scroll_offset: Vec2,
    ) -> bool {
        let mut locations = self.locations.get_untracked();
        if let Some(last_location) = locations.last() {
            if last_location.path == path
                && last_location.position == Some(EditorPosition::Offset(offset))
                && last_location.scroll_offset == Some(scroll_offset)
            {
                return false;
            }
        }
        let location = EditorLocation {
            path,
            position: Some(EditorPosition::Offset(offset)),
            scroll_offset: Some(scroll_offset),
            ignore_unconfirmed: false,
            same_editor_tab: false,
        };
        locations.push_back(location.clone());
        let current_location = locations.len();
        self.locations.set(locations);
        self.current_location.set(current_location);

        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        if let Some((locations, current_location)) = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .map(|editor_tab| {
                editor_tab.with_untracked(|editor_tab| {
                    (editor_tab.locations, editor_tab.current_location)
                })
            })
        {
            let mut l = locations.get_untracked();
            l.push_back(location);
            let new_current_location = l.len();
            locations.set(l);
            current_location.set(new_current_location);
        }

        true
    }

    pub fn jump_to_location(
        &self,
        location: EditorLocation,
        edits: Option<Vec<TextEdit>>,
    ) {
        self.save_current_jump_location();
        self.go_to_location(location, edits);
    }

    pub fn get_doc(
        &self,
        path: PathBuf,
        unsaved: Option<String>,
    ) -> (Rc<Doc>, bool) {
        let cx = self.scope;
        let doc = self.docs.with_untracked(|docs| docs.get(&path).cloned());
        if let Some(doc) = doc {
            (doc, false)
        } else {
            let diagnostic_data = self.get_diagnostic_data(&path);

            let doc = Doc::new(
                cx,
                path.clone(),
                diagnostic_data,
                self.editors,
                self.common.clone(),
            );
            let doc = Rc::new(doc);
            self.docs.update(|docs| {
                docs.insert(path.clone(), doc.clone());
            });

            {
                let doc = doc.clone();
                let local_doc = doc.clone();
                let send = create_ext_action(cx, move |result| {
                    if let Ok(ProxyResponse::NewBufferResponse {
                        content,
                        read_only,
                    }) = result
                    {
                        local_doc.init_content(Rope::from(content));
                        if read_only {
                            local_doc.content.update(|content| {
                                if let DocContent::File { read_only, .. } = content {
                                    *read_only = true;
                                }
                            });
                        } else if let Some(unsaved) = unsaved {
                            local_doc.reload(Rope::from(unsaved), false);
                        }
                    }
                });

                self.common
                    .proxy
                    .new_buffer(doc.buffer_id, path, move |result| {
                        send(result);
                    });
            }
            doc.get_code_lens();
            doc.get_folding_range();
            doc.get_document_symbol();
            (doc, true)
        }
    }

    pub fn go_to_location(
        &self,
        location: EditorLocation,
        edits: Option<Vec<TextEdit>>,
    ) {
        if self.common.focus.get_untracked() != Focus::Workbench {
            self.common.focus.set(Focus::Workbench);
        }
        let path = location.path.clone();
        let (doc, new_doc) = self.get_doc(path.clone(), None);

        let child = self.get_editor_tab_child(
            EditorTabChildSource::Editor { path, doc },
            location.ignore_unconfirmed,
            location.same_editor_tab,
        );
        if let EditorTabChild::Editor(editor_id) = child {
            if let Some(editor) = self.editors.editor_untracked(editor_id) {
                editor.go_to_location(location, new_doc, edits);
            }
        }
    }

    pub fn open_file_changes(&self, path: PathBuf) {
        let (right, _) = self.get_doc(path.clone(), None);
        let left = Doc::new_history(
            self.scope,
            DocContent::History(DocHistory {
                path: path.clone(),
                version: "head".to_string(),
            }),
            self.editors,
            self.common.clone(),
        );
        let left = Rc::new(left);

        let send = {
            let left = left.clone();
            create_ext_action(self.scope, move |result| {
                if let Ok(ProxyResponse::BufferHeadResponse { content, .. }) = result
                {
                    left.init_content(Rope::from(content));
                }
            })
        };
        self.common.proxy.get_buffer_head(path, move |result| {
            send(result);
        });

        self.get_editor_tab_child(
            EditorTabChildSource::DiffEditor { left, right },
            false,
            false,
        );
    }

    pub fn open_diff_files(&self, left_path: PathBuf, right_path: PathBuf) {
        let [left, right] =
            [left_path, right_path].map(|path| self.get_doc(path, None).0);

        self.get_editor_tab_child(
            EditorTabChildSource::DiffEditor { left, right },
            false,
            false,
        );
    }

    fn new_editor_tab(
        &self,
        editor_tab_id: EditorTabId,
        split_id: SplitId,
    ) -> RwSignal<EditorTabData> {
        let editor_tab = {
            let cx = self.scope.create_child();
            let editor_tab = EditorTabData {
                scope: cx,
                split: split_id,
                active: 0,
                editor_tab_id,
                children: vec![],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: cx.create_rw_signal(im::Vector::new()),
                current_location: cx.create_rw_signal(0),
            };
            cx.create_rw_signal(editor_tab)
        };
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        editor_tab
    }

    fn get_editor_tab_child(
        &self,
        source: EditorTabChildSource,
        ignore_unconfirmed: bool,
        same_editor_tab: bool,
    ) -> EditorTabChild {
        let config = self.common.config.get_untracked();

        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        let editors = self.editors;
        let diff_editors = self.diff_editors.get_untracked();

        let active_editor_tab = if let Some(editor_tab) = active_editor_tab {
            editor_tab
        } else if editor_tabs.is_empty() {
            let editor_tab_id = EditorTabId::next();
            let editor_tab = self.new_editor_tab(editor_tab_id, self.root_split);
            let root_split = self.splits.with_untracked(|splits| {
                splits.get(&self.root_split).cloned().unwrap()
            });
            root_split.update(|root_split| {
                root_split.children = vec![(
                    root_split.scope.create_rw_signal(1.0),
                    SplitContent::EditorTab(editor_tab_id),
                )];
            });
            self.active_editor_tab.set(Some(editor_tab_id));
            editor_tab
        } else {
            let (editor_tab_id, editor_tab) = editor_tabs.iter().next().unwrap();
            self.active_editor_tab.set(Some(*editor_tab_id));
            *editor_tab
        };

        let is_same_diff_editor =
            |diff_editor_id: &DiffEditorId, left: &Rc<Doc>, right: &Rc<Doc>| {
                diff_editors
                    .get(diff_editor_id)
                    .map(|diff_editor| {
                        left.content.get_untracked()
                            == diff_editor.left.doc().content.get_untracked()
                            && right.content.get_untracked()
                                == diff_editor.right.doc().content.get_untracked()
                    })
                    .unwrap_or(false)
            };

        let selected = if !config.editor.show_tab {
            active_editor_tab.with_untracked(|editor_tab| {
                for (i, (_, _, child)) in editor_tab.children.iter().enumerate() {
                    let can_be_selected = match child {
                        EditorTabChild::Editor(editor_id) => {
                            if let Some(editor) =
                                editors.editor_untracked(*editor_id)
                            {
                                let doc = editor.doc();
                                let same_path =
                                    if let EditorTabChildSource::Editor {
                                        path,
                                        ..
                                    } = &source
                                    {
                                        doc.content.with_untracked(|content| {
                                            content
                                                .path()
                                                .map(|p| p == path)
                                                .unwrap_or(false)
                                        })
                                    } else {
                                        false
                                    };

                                same_path
                                    || doc.buffer.with_untracked(|b| b.is_pristine())
                            } else {
                                false
                            }
                        }
                        EditorTabChild::DiffEditor(diff_editor_id) => {
                            if let EditorTabChildSource::DiffEditor { left, right } =
                                &source
                            {
                                is_same_diff_editor(diff_editor_id, left, right)
                                    || diff_editors
                                        .get(diff_editor_id)
                                        .map(|diff_editor| {
                                            diff_editor.left.doc().is_pristine()
                                                && diff_editor
                                                    .right
                                                    .doc()
                                                    .is_pristine()
                                        })
                                        .unwrap_or(false)
                            } else {
                                false
                            }
                        }
                        EditorTabChild::Settings(_) => true,
                        EditorTabChild::ThemeColorSettings(_) => true,
                        EditorTabChild::Keymap(_) => true,
                        EditorTabChild::Volt(_, _) => true,
                    };

                    if can_be_selected {
                        return Some(i);
                    }
                }
                None
            })
        } else {
            match &source {
                EditorTabChildSource::Editor { path, .. } => active_editor_tab
                    .with_untracked(|editor_tab| {
                        editor_tab
                            .get_editor(editors, path)
                            .map(|(i, _)| i)
                            .or_else(|| {
                                if ignore_unconfirmed {
                                    None
                                } else {
                                    editor_tab
                                        .get_unconfirmed_editor_tab_child(
                                            editors,
                                            &diff_editors,
                                        )
                                        .map(|(i, _)| i)
                                }
                            })
                    }),
                EditorTabChildSource::DiffEditor { left, right } => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, _, child)| {
                                if let EditorTabChild::DiffEditor(diff_editor_id) =
                                    child
                                {
                                    is_same_diff_editor(diff_editor_id, left, right)
                                } else {
                                    false
                                }
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
                EditorTabChildSource::NewFileEditor => {
                    if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
                EditorTabChildSource::Settings => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, _, child)| {
                                matches!(child, EditorTabChild::Settings(_))
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
                EditorTabChildSource::ThemeColorSettings => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, _, child)| {
                                matches!(
                                    child,
                                    EditorTabChild::ThemeColorSettings(_)
                                )
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
                EditorTabChildSource::Keymap => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, _, child)| {
                                matches!(child, EditorTabChild::Keymap(_))
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
                EditorTabChildSource::Volt(id) => {
                    if let Some(index) =
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab.children.iter().position(|(_, _, child)| {
                                if let EditorTabChild::Volt(_, current_id) = child {
                                    current_id == id
                                } else {
                                    false
                                }
                            })
                        })
                    {
                        Some(index)
                    } else if ignore_unconfirmed {
                        None
                    } else {
                        active_editor_tab.with_untracked(|editor_tab| {
                            editor_tab
                                .get_unconfirmed_editor_tab_child(
                                    editors,
                                    &diff_editors,
                                )
                                .map(|(i, _)| i)
                        })
                    }
                }
            }
        };

        let new_child_from_source =
            |editor_tab_id: EditorTabId, source: &EditorTabChildSource| match source
            {
                EditorTabChildSource::Editor { doc, .. } => {
                    let editor_id = editors.new_from_doc(
                        self.scope,
                        doc.clone(),
                        Some(editor_tab_id),
                        None,
                        None,
                        self.common.clone(),
                    );

                    EditorTabChild::Editor(editor_id)
                }
                EditorTabChildSource::NewFileEditor => {
                    let name = self.get_name_for_new_file();
                    let doc_content = DocContent::Scratch {
                        id: BufferId::next(),
                        name: name.clone(),
                    };
                    let doc = Doc::new_content(
                        self.scope,
                        doc_content,
                        self.editors,
                        self.common.clone(),
                    );
                    let doc = Rc::new(doc);
                    self.scratch_docs.update(|scratch_docs| {
                        scratch_docs.insert(name, doc.clone());
                    });
                    let editor_id = editors.new_from_doc(
                        self.scope,
                        doc,
                        Some(editor_tab_id),
                        None,
                        None,
                        self.common.clone(),
                    );

                    EditorTabChild::Editor(editor_id)
                }
                EditorTabChildSource::Settings => {
                    EditorTabChild::Settings(SettingsId::next())
                }
                EditorTabChildSource::ThemeColorSettings => {
                    EditorTabChild::ThemeColorSettings(SettingsId::next())
                }
                EditorTabChildSource::Keymap => {
                    EditorTabChild::Keymap(KeymapId::next())
                }
                EditorTabChildSource::Volt(id) => {
                    EditorTabChild::Volt(VoltViewId::next(), id.to_owned())
                }
                EditorTabChildSource::DiffEditor { left, right } => {
                    let diff_editor_id = DiffEditorId::next();
                    let diff_editor = DiffEditorData::new(
                        self.scope,
                        diff_editor_id,
                        editor_tab_id,
                        left.clone(),
                        right.clone(),
                        editors,
                        self.common.clone(),
                    );
                    self.diff_editors.update(|diff_editors| {
                        diff_editors.insert(diff_editor_id, diff_editor);
                    });
                    EditorTabChild::DiffEditor(diff_editor_id)
                }
            };

        if let Some(selected) = selected {
            let (editor_tab_id, current_child) =
                active_editor_tab.with_untracked(|editor_tab| {
                    let editor_tab_id = editor_tab.editor_tab_id;
                    let (_, _, current_child) = &editor_tab.children[selected];
                    match current_child {
                        EditorTabChild::Editor(editor_id) => {
                            if let Some(editor) =
                                editors.editor_untracked(*editor_id)
                            {
                                editor.save_doc_position();
                            }
                        }
                        EditorTabChild::DiffEditor(_) => {}
                        EditorTabChild::Settings(_) => {}
                        EditorTabChild::ThemeColorSettings(_) => {}
                        EditorTabChild::Keymap(_) => {}
                        EditorTabChild::Volt(_, _) => {}
                    }
                    (editor_tab_id, current_child.clone())
                });

            // firstly, if they are the same type of child, load the new doc to the old editor
            let is_same = match (&current_child, &source) {
                (
                    EditorTabChild::Editor(editor_id),
                    EditorTabChildSource::Editor { path, doc },
                ) => {
                    if let Some(editor) = editors.editor_untracked(*editor_id) {
                        let same_path = editor
                            .doc()
                            .content
                            .with_untracked(|content| content.path() == Some(path));
                        if !same_path {
                            editor.update_doc(doc.clone());
                            editor.cursor().set(Cursor::origin(
                                self.common.config.with_untracked(|c| c.core.modal),
                            ));
                        }
                    }

                    true
                }
                (
                    EditorTabChild::DiffEditor(diff_editor_id),
                    EditorTabChildSource::DiffEditor { left, right },
                ) => {
                    if !is_same_diff_editor(diff_editor_id, left, right) {
                        if let Some(diff_editor) = diff_editors.get(diff_editor_id) {
                            diff_editor.left.update_doc(left.clone());
                            diff_editor.right.update_doc(right.clone());
                        }
                    }
                    true
                }
                (EditorTabChild::Settings(_), EditorTabChildSource::Settings) => {
                    true
                }
                _ => false,
            };
            if is_same {
                let (_, _, child) = active_editor_tab.with_untracked(|editor_tab| {
                    editor_tab.children[selected].clone()
                });
                active_editor_tab.update(|editor_tab| {
                    editor_tab.active = selected;
                });
                return child;
            }

            // We're loading a different kind of child, clean up the old resources
            match &current_child {
                EditorTabChild::Editor(editor_id) => {
                    self.remove_editor(*editor_id);
                }
                EditorTabChild::DiffEditor(diff_editor_id) => {
                    self.diff_editors.update(|diff_editors| {
                        diff_editors.remove(diff_editor_id);
                    });
                }
                EditorTabChild::Settings(_) => {}
                EditorTabChild::ThemeColorSettings(_) => {}
                EditorTabChild::Keymap(_) => {}
                EditorTabChild::Volt(_, _) => {}
            }

            // Now loading the new child
            let child = new_child_from_source(editor_tab_id, &source);
            active_editor_tab.update(|editor_tab| {
                editor_tab.children[selected] = (
                    editor_tab.scope.create_rw_signal(0),
                    editor_tab.scope.create_rw_signal(Rect::ZERO),
                    child.clone(),
                );
                editor_tab.active = selected;
            });
            return child;
        }

        // check file exists in non active editor tabs
        if config.editor.show_tab && !ignore_unconfirmed && !same_editor_tab {
            for (editor_tab_id, editor_tab) in &editor_tabs {
                if Some(*editor_tab_id) != active_editor_tab_id {
                    if let Some(index) =
                        editor_tab.with_untracked(|editor_tab| match &source {
                            EditorTabChildSource::Editor { path, .. } => editor_tab
                                .get_editor(editors, path)
                                .map(|(index, _)| index),
                            EditorTabChildSource::DiffEditor { left, right } => {
                                editor_tab.children.iter().position(
                                    |(_, _, child)| {
                                        if let EditorTabChild::DiffEditor(
                                            diff_editor_id,
                                        ) = child
                                        {
                                            is_same_diff_editor(
                                                diff_editor_id,
                                                left,
                                                right,
                                            )
                                        } else {
                                            false
                                        }
                                    },
                                )
                            }
                            EditorTabChildSource::Settings => editor_tab
                                .children
                                .iter()
                                .position(|(_, _, child)| {
                                    matches!(child, EditorTabChild::Settings(_))
                                }),
                            EditorTabChildSource::ThemeColorSettings => editor_tab
                                .children
                                .iter()
                                .position(|(_, _, child)| {
                                    matches!(
                                        child,
                                        EditorTabChild::ThemeColorSettings(_)
                                    )
                                }),
                            EditorTabChildSource::Keymap => editor_tab
                                .children
                                .iter()
                                .position(|(_, _, child)| {
                                    matches!(child, EditorTabChild::Keymap(_))
                                }),
                            EditorTabChildSource::Volt(id) => editor_tab
                                .children
                                .iter()
                                .position(|(_, _, child)| {
                                    if let EditorTabChild::Volt(_, current_id) =
                                        child
                                    {
                                        current_id == id
                                    } else {
                                        false
                                    }
                                }),
                            EditorTabChildSource::NewFileEditor => None,
                        })
                    {
                        self.active_editor_tab.set(Some(*editor_tab_id));
                        editor_tab.update(|editor_tab| {
                            editor_tab.active = index;
                        });
                        let (_, _, child) =
                            editor_tab.with_untracked(|editor_tab| {
                                editor_tab.children[index].clone()
                            });
                        return child;
                    }
                }
            }
        }

        let editor_tab_id =
            active_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
        let child = new_child_from_source(editor_tab_id, &source);

        active_editor_tab.update(|editor_tab| {
            let active = editor_tab
                .active
                .min(editor_tab.children.len().saturating_sub(1));
            let new_active = if editor_tab.children.is_empty() {
                0
            } else {
                active + 1
            };
            editor_tab.children.insert(
                new_active,
                (
                    self.scope.create_rw_signal(0),
                    self.scope.create_rw_signal(Rect::ZERO),
                    child.clone(),
                ),
            );
            editor_tab.active = new_active;
        });

        child
    }

    pub fn remove_editor(&self, editor_id: EditorId) {
        if let Some(editor) = self.editors.remove(editor_id) {
            editor.save_doc_position();

            let doc = editor.doc();
            let (content, _) = (doc.content.get_untracked(), doc.is_pristine());
            if let DocContent::Scratch { name, .. } = content {
                let doc_exists = self.editors.with_editors_untracked(|editors| {
                    editors.iter().any(|(_, editor_data)| {
                        let doc = editor_data.doc();

                        if let DocContent::Scratch {
                            name: current_name, ..
                        } = &doc.content.get_untracked()
                        {
                            current_name == &name
                        } else {
                            false
                        }
                    })
                });
                if !doc_exists {
                    self.scratch_docs.update(|scratch_docs| {
                        scratch_docs.remove(&name);
                    });
                }
            }
        }
    }

    pub fn jump_location_backward(&self, local: bool) {
        let (locations, current_location) = if local {
            let active_editor_tab_id = self.active_editor_tab.get_untracked();
            let editor_tabs = self.editor_tabs.get_untracked();
            if let Some((locations, current_location)) = active_editor_tab_id
                .and_then(|id| editor_tabs.get(&id))
                .map(|editor_tab| {
                    editor_tab.with_untracked(|editor_tab| {
                        (editor_tab.locations, editor_tab.current_location)
                    })
                })
            {
                (locations, current_location)
            } else {
                return;
            }
        } else {
            (self.locations, self.current_location)
        };

        let locations_value = locations.get_untracked();
        let current_location_value = current_location.get_untracked();
        if current_location_value < 1 {
            return;
        }

        if current_location_value >= locations_value.len() {
            // if we are at the head of the locations, save the current location
            // before jump back
            if self.save_current_jump_location() {
                current_location.update(|l| {
                    *l -= 1;
                });
            }
        }

        current_location.update(|l| {
            *l -= 1;
        });
        let current_location_value = current_location.get_untracked();
        current_location.set(current_location_value);
        let mut location = locations_value[current_location_value].clone();
        // for local jumps, we keep on the same editor tab
        // because we only jump on the same split
        location.same_editor_tab = local;

        self.go_to_location(location, None);
    }

    pub fn jump_location_forward(&self, local: bool) {
        let (locations, current_location) = if local {
            let active_editor_tab_id = self.active_editor_tab.get_untracked();
            let editor_tabs = self.editor_tabs.get_untracked();
            if let Some((locations, current_location)) = active_editor_tab_id
                .and_then(|id| editor_tabs.get(&id))
                .map(|editor_tab| {
                    editor_tab.with_untracked(|editor_tab| {
                        (editor_tab.locations, editor_tab.current_location)
                    })
                })
            {
                (locations, current_location)
            } else {
                return;
            }
        } else {
            (self.locations, self.current_location)
        };

        let locations_value = locations.get_untracked();
        let current_location_value = current_location.get_untracked();
        if locations_value.is_empty() {
            return;
        }
        if current_location_value >= locations_value.len() - 1 {
            return;
        }
        current_location.set(current_location_value + 1);
        let mut location = locations_value[current_location_value + 1].clone();
        // for local jumps, we keep on the same editor tab
        // because we only jump on the same split
        location.same_editor_tab = local;
        self.go_to_location(location, None);
    }

    pub fn split(
        &self,
        direction: SplitDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        let split_direction = split.with_untracked(|split| split.direction);

        let (index, children_len) = split.with_untracked(|split| {
            split
                .children
                .iter()
                .position(|(_, c)| c == &SplitContent::EditorTab(editor_tab_id))
                .map(|index| (index, split.children.len()))
        })?;

        if split_direction == direction {
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(self.scope, split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
            split.update(|split| {
                for (size, _) in &split.children {
                    size.set(1.0);
                }
                split.children.insert(
                    index + 1,
                    (
                        split.scope.create_rw_signal(1.0),
                        SplitContent::EditorTab(new_editor_tab_id),
                    ),
                );
            });
        } else if children_len == 1 {
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(self.scope, split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);
            split.update(|split| {
                split.direction = direction;
                for (size, _) in &split.children {
                    size.set(1.0);
                }
                split.children.push((
                    split.scope.create_rw_signal(1.0),
                    SplitContent::EditorTab(new_editor_tab_id),
                ));
            });
        } else {
            let new_split_id = SplitId::next();

            editor_tab.update(|editor_tab| {
                editor_tab.split = new_split_id;
            });
            let new_editor_tab = editor_tab.with_untracked(|editor_tab| {
                self.split_editor_tab(self.scope, new_split_id, editor_tab)
            })?;
            let new_editor_tab_id =
                new_editor_tab.with_untracked(|editor_tab| editor_tab.editor_tab_id);

            let new_split = {
                let cx = self.scope.create_child();
                let new_split = SplitData {
                    scope: cx,
                    parent_split: Some(split_id),
                    split_id: new_split_id,
                    children: vec![
                        (
                            cx.create_rw_signal(1.0),
                            SplitContent::EditorTab(editor_tab_id),
                        ),
                        (
                            cx.create_rw_signal(1.0),
                            SplitContent::EditorTab(new_editor_tab_id),
                        ),
                    ],
                    direction,
                    window_origin: Point::ZERO,
                    layout_rect: Rect::ZERO,
                };
                cx.create_rw_signal(new_split)
            };
            self.splits.update(|splits| {
                splits.insert(new_split_id, new_split);
            });
            split.update(|split| {
                let size = split.children[index].0.get_untracked();
                split.children[index] = (
                    split.scope.create_rw_signal(size),
                    SplitContent::Split(new_split_id),
                );
            });
        }

        Some(())
    }

    fn split_editor_tab(
        &self,
        cx: Scope,
        split_id: SplitId,
        editor_tab: &EditorTabData,
    ) -> Option<RwSignal<EditorTabData>> {
        let (_, _, child) = editor_tab.children.get(editor_tab.active)?;

        let editor_tab_id = EditorTabId::next();

        let new_child = match child {
            EditorTabChild::Editor(editor_id) => {
                let editor_id = self
                    .editors
                    .copy(*editor_id, cx, Some(editor_tab_id), None, None)
                    .unwrap();

                EditorTabChild::Editor(editor_id)
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let new_diff_editor_id = DiffEditorId::next();
                let diff_editor = self
                    .diff_editors
                    .get_untracked()
                    .get(diff_editor_id)?
                    .copy(cx, editor_tab_id, new_diff_editor_id, self.editors);
                self.diff_editors.update(|diff_editors| {
                    diff_editors.insert(new_diff_editor_id, diff_editor);
                });
                EditorTabChild::DiffEditor(new_diff_editor_id)
            }
            EditorTabChild::Settings(_) => {
                EditorTabChild::Settings(SettingsId::next())
            }
            EditorTabChild::ThemeColorSettings(_) => {
                EditorTabChild::ThemeColorSettings(ThemeColorSettingsId::next())
            }
            EditorTabChild::Keymap(_) => EditorTabChild::Keymap(KeymapId::next()),
            EditorTabChild::Volt(_, id) => {
                EditorTabChild::Volt(VoltViewId::next(), id.to_owned())
            }
        };

        let editor_tab = {
            let cx = self.scope.create_child();
            let editor_tab = EditorTabData {
                scope: cx,
                split: split_id,
                editor_tab_id,
                active: 0,
                children: vec![(
                    cx.create_rw_signal(0),
                    cx.create_rw_signal(Rect::ZERO),
                    new_child,
                )],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: cx.create_rw_signal(editor_tab.locations.get_untracked()),
                current_location: cx
                    .create_rw_signal(editor_tab.current_location.get_untracked()),
            };
            cx.create_rw_signal(editor_tab)
        };
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.insert(editor_tab_id, editor_tab);
        });
        Some(editor_tab)
    }

    pub fn split_move(
        &self,
        direction: SplitMoveDirection,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let rect = editor_tab.with_untracked(|editor_tab| {
            editor_tab.layout_rect.with_origin(editor_tab.window_origin)
        });

        match direction {
            SplitMoveDirection::Up => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (current_rect.y1 - rect.y0).abs() < 3.0
                        && current_rect.x0 <= rect.x0
                        && rect.x0 < current_rect.x1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Down => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (current_rect.y0 - rect.y1).abs() < 3.0
                        && current_rect.x0 <= rect.x0
                        && rect.x0 < current_rect.x1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Right => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (rect.x1 - current_rect.x0).abs() < 3.0
                        && current_rect.y0 <= rect.y0
                        && rect.y0 < current_rect.y1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
            SplitMoveDirection::Left => {
                for (_, e) in editor_tabs.iter() {
                    let current_rect = e.with_untracked(|e| {
                        e.layout_rect.with_origin(e.window_origin)
                    });
                    if (current_rect.x1 - rect.x0).abs() < 3.0
                        && current_rect.y0 <= rect.y0
                        && rect.y0 < current_rect.y1
                    {
                        let new_editor_tab_id =
                            e.with_untracked(|e| e.editor_tab_id);
                        self.active_editor_tab.set(Some(new_editor_tab_id));
                        return Some(());
                    }
                }
            }
        }

        Some(())
    }

    pub fn split_exchange(&self, editor_tab_id: EditorTabId) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        split.update(|split| {
            let index = split
                .children
                .iter()
                .position(|(_, c)| c == &SplitContent::EditorTab(editor_tab_id));
            if let Some(index) = index {
                if index < split.children.len() - 1 {
                    split.children.swap(index, index + 1);
                }
                self.split_content_focus(&split.children[index].1);
            }
        });

        Some(())
    }

    fn split_content_focus(&self, content: &SplitContent) {
        match content {
            SplitContent::EditorTab(editor_tab_id) => {
                self.active_editor_tab.set(Some(*editor_tab_id));
            }
            SplitContent::Split(split_id) => {
                self.split_focus(*split_id);
            }
        }
    }

    fn split_focus(&self, split_id: SplitId) -> Option<()> {
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;

        let split_chilren = split.with_untracked(|split| split.children.clone());
        let content = split_chilren.first()?;
        self.split_content_focus(&content.1);

        Some(())
    }

    fn split_remove(&self, split_id: SplitId) -> Option<()> {
        if split_id == self.root_split {
            return Some(());
        }

        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        self.splits.update(|splits| {
            splits.remove(&split_id);
        });
        let parent_split_id = split.with_untracked(|split| split.parent_split)?;
        let parent_split = splits.get(&parent_split_id).copied()?;

        let split_len = parent_split
            .try_update(|split| {
                split
                    .children
                    .retain(|(_, c)| c != &SplitContent::Split(split_id));
                split.children.len()
            })
            .unwrap();
        if split_len == 0 {
            self.split_remove(parent_split_id);
        } else if split_len == 1 {
            let parent_parent_split_id =
                parent_split.with_untracked(|split| split.parent_split)?;
            let parent_parent_split =
                splits.get(&parent_parent_split_id).copied()?;
            let parent_split_index =
                parent_parent_split.with_untracked(|split| {
                    split.content_index(&SplitContent::Split(parent_split_id))
                })?;
            let orphan = parent_split
                .try_update(|split| split.children.remove(0))
                .unwrap();
            self.split_content_set_parent(&orphan.1, parent_parent_split_id);
            parent_parent_split.update(|parent_parent_split| {
                parent_parent_split.children[parent_split_index] = orphan;
            });
            self.split_remove(parent_split_id);
        }

        Some(())
    }

    fn editor_tab_remove(&self, editor_tab_id: EditorTabId) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        self.editor_tabs.update(|editor_tabs| {
            editor_tabs.remove(&editor_tab_id);
        });

        let split_id = editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let splits = self.splits.get_untracked();
        let split = splits.get(&split_id).copied()?;
        let parent_split_id = split.with_untracked(|split| split.parent_split);

        let is_active = self
            .active_editor_tab
            .with_untracked(|active| active == &Some(editor_tab_id));

        let index = split.with_untracked(|split| {
            split
                .children
                .iter()
                .position(|(_, c)| c == &SplitContent::EditorTab(editor_tab_id))
        })?;
        split.update(|split| {
            split.children.remove(index);
            for (size, _) in &split.children {
                size.set(1.0);
            }
        });
        let split_children = split.with_untracked(|split| split.children.clone());

        if is_active {
            if split_children.is_empty() {
                let new_focus = parent_split_id
                    .and_then(|split_id| splits.get(&split_id))
                    .and_then(|split| {
                        let index = split.with_untracked(|split| {
                            split.children.iter().position(|(_, c)| {
                                c == &SplitContent::Split(split_id)
                            })
                        })?;
                        Some((index, split))
                    })
                    .and_then(|(index, split)| {
                        split.with_untracked(|split| {
                            if index == split.children.len() - 1 {
                                if index > 0 {
                                    Some(split.children[index - 1])
                                } else {
                                    None
                                }
                            } else {
                                Some(split.children[index + 1])
                            }
                        })
                    });
                if let Some((_, content)) = new_focus {
                    self.split_content_focus(&content);
                }
            } else {
                let (_, content) =
                    split_children[index.min(split_children.len() - 1)];
                self.split_content_focus(&content);
            }
        }

        if split_children.is_empty() {
            self.split_remove(split_id);
        } else if split_children.len() == 1 {
            if let Some(parent_split_id) = parent_split_id {
                let parent_split = splits.get(&parent_split_id).copied()?;
                let split_index = parent_split.with_untracked(|split| {
                    split.content_index(&SplitContent::Split(split_id))
                })?;
                let (_, orphan) =
                    split.try_update(|split| split.children.remove(0)).unwrap();
                self.split_content_set_parent(&orphan, parent_split_id);
                parent_split.update(|parent_split| {
                    let size = parent_split.children[split_index].0.get_untracked();
                    parent_split.children[split_index] =
                        (parent_split.scope.create_rw_signal(size), orphan);
                });
                self.split_remove(split_id);
            }
        }

        Some(())
    }

    pub fn editor_tab_close(&self, editor_tab_id: EditorTabId) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
        let editor_tab = editor_tab.get_untracked();
        for (_, _, child) in editor_tab.children {
            self.editor_tab_child_close(editor_tab_id, child, false);
        }

        Some(())
    }

    fn editor_tab_child_close_warning(
        &self,
        child: &EditorTabChild,
    ) -> Option<(String, Rc<Doc>, EditorData)> {
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self.editors.editor_untracked(*editor_id)?;
                let doc = editor.doc();
                let doc_content = doc.content.get_untracked();
                let is_dirty = !doc.is_pristine();
                if is_dirty {
                    let exists = self.editors.with_editors_untracked(|editors| {
                        editors.iter().any(|(id, editor)| {
                            let doc = editor.doc();
                            id != editor_id
                                && doc.content.with_untracked(|content| {
                                    content == &doc_content
                                })
                        })
                    });
                    if !exists {
                        return match doc_content {
                            DocContent::File { path, .. } => Some((
                                path.file_name()?.to_str()?.to_string(),
                                doc,
                                editor,
                            )),
                            DocContent::Local => None,
                            DocContent::History(_) => None,
                            DocContent::Scratch { name, .. } => {
                                Some((name, doc, editor))
                            }
                        };
                    }
                }
                None
            }
            EditorTabChild::DiffEditor(_) => None,
            EditorTabChild::Settings(_) => None,
            EditorTabChild::ThemeColorSettings(_) => None,
            EditorTabChild::Keymap(_) => None,
            EditorTabChild::Volt(_, _) => None,
        }
    }

    pub fn split_exchange_active(&self) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        self.split_exchange(active_editor_tab)?;
        Some(())
    }

    pub fn split_move_active(&self, direction: SplitMoveDirection) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        self.split_move(direction, active_editor_tab)?;
        Some(())
    }

    pub fn split_active(&self, direction: SplitDirection) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        self.split(direction, active_editor_tab)?;
        Some(())
    }

    pub fn editor_tab_child_close_active(&self) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let (_, _, child) = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        self.editor_tab_child_close(active_editor_tab, child, false);
        Some(())
    }

    pub fn editor_tab_child_close_by_kind(
        &self,
        editor_tab_id: EditorTabId,
        child: EditorTabChild,
        kind: TabCloseKind,
    ) -> Option<()> {
        let tabs_to_close: Vec<EditorTabChild> = {
            let editor_tabs = self.editor_tabs.get_untracked();

            let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;
            let editor_tab = editor_tab.get_untracked();
            match kind {
                TabCloseKind::CloseOther => editor_tab
                    .children
                    .iter()
                    .filter_map(|x| {
                        if x.2 != child {
                            Some(x.2.clone())
                        } else {
                            None
                        }
                    })
                    .collect(),
                TabCloseKind::CloseToLeft => {
                    let mut tabs_to_close = Vec::new();
                    for child_tab in &editor_tab.children {
                        if child_tab.2 != child {
                            tabs_to_close.push(child_tab.2.clone());
                        } else {
                            break;
                        }
                    }
                    tabs_to_close
                }
                TabCloseKind::CloseToRight => {
                    let mut tabs_to_close = Vec::new();
                    let mut add_to_tabs = false;
                    for child_tab in &editor_tab.children {
                        if child_tab.2 != child && add_to_tabs {
                            tabs_to_close.push(child_tab.2.clone());
                        } else {
                            add_to_tabs = true;
                        }
                    }
                    tabs_to_close
                }
            }
        };
        for child_tab in tabs_to_close {
            self.editor_tab_child_close(editor_tab_id, child_tab, false);
        }
        Some(())
    }

    pub fn editor_tab_child_close(
        &self,
        editor_tab_id: EditorTabId,
        child: EditorTabChild,
        force: bool,
    ) -> Option<()> {
        if !force {
            if let Some((name, doc, editor)) =
                self.editor_tab_child_close_warning(&child)
            {
                let internal_command = self.common.internal_command;
                let main_split = self.clone();

                let doc_content = doc.content.get_untracked();
                let save_button = match doc_content {
                    DocContent::Scratch { .. } => {
                        let child = child.clone();
                        let doc = doc.clone();
                        let save_action = Rc::new(move || {
                            let child = child.clone();
                            let main_split = main_split.clone();
                            let doc = doc.clone();
                            internal_command.send(InternalCommand::HideAlert);
                            save_as(
                                FileDialogOptions::new().title("Save File"),
                                move |file: Option<FileInfo>| {
                                    let main_split = main_split.clone();
                                    let child = child.clone();
                                    let local_main_split = main_split.clone();
                                    if let Some(mut file) = file {
                                        main_split.save_as(
                                            doc.clone(),
                                            if let Some(path) = file.path.pop() {
                                                path
                                            } else {
                                                tracing::error!("No path");
                                                return;
                                            },
                                            move || {
                                                local_main_split
                                                    .clone()
                                                    .editor_tab_child_close(
                                                        editor_tab_id,
                                                        child.clone(),
                                                        false,
                                                    );
                                            },
                                        );
                                    }
                                },
                            );
                        });
                        Some(AlertButton {
                            text: "Save".to_string(),
                            action: save_action,
                        })
                    }
                    DocContent::File { .. } => {
                        let editor = editor.clone();
                        let editors = self.editors;
                        let editor_id = editor.id();
                        let save_action = Rc::new(move || {
                            internal_command.send(InternalCommand::HideAlert);
                            editor.save(false, move || {
                                if let Some(editor) =
                                    editors.editor_untracked(editor_id)
                                {
                                    editor.clone().run_focus_command(
                                        &FocusCommand::SplitClose,
                                        None,
                                        Modifiers::empty(),
                                    );
                                }
                            });
                        });
                        Some(AlertButton {
                            text: "Save".to_string(),
                            action: save_action,
                        })
                    }
                    DocContent::Local => None,
                    DocContent::History(_) => None,
                };
                if let Some(save_button) = save_button {
                    let main_split = self.clone();
                    let child = child.clone();
                    self.common
                        .internal_command
                        .send(InternalCommand::ShowAlert {
                            title: format!(
                                "Do you want to save the changes you made to {name}?"
                            ),
                            msg: "Your changes will be lost if you don't save them."
                                .to_string(),
                            buttons: vec![
                                save_button,
                                AlertButton {
                                    text: "Don't Save".to_string(),
                                    action: Rc::new(move || {
                                        internal_command
                                            .send(InternalCommand::HideAlert);
                                        main_split.editor_tab_child_close(
                                            editor_tab_id,
                                            child.clone(),
                                            true,
                                        );
                                    }),
                                },
                            ],
                        });
                }

                return Some(());
            }
        }

        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(&editor_tab_id).copied()?;

        let index = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.iter().position(|(_, _, c)| c == &child)
        })?;

        let editor_tab_children_len = editor_tab
            .try_update(|editor_tab| {
                editor_tab.children.remove(index);
                editor_tab.active =
                    index.min(editor_tab.children.len().saturating_sub(1));
                editor_tab.children.len()
            })
            .unwrap();

        match child {
            EditorTabChild::Editor(editor_id) => {
                self.remove_editor(editor_id);
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let removed_diff_editor = self
                    .diff_editors
                    .try_update(|diff_editors| diff_editors.remove(&diff_editor_id))
                    .unwrap();
                if let Some(diff_editor) = removed_diff_editor {
                    diff_editor.right.save_doc_position();
                    self.editors.remove(diff_editor.right.editor.id());
                    self.editors.remove(diff_editor.left.editor.id());
                }
            }
            EditorTabChild::Settings(_) => {}
            EditorTabChild::ThemeColorSettings(_) => {}
            EditorTabChild::Keymap(_) => {}
            EditorTabChild::Volt(_, _) => {}
        }

        if editor_tab_children_len == 0 {
            self.editor_tab_remove(editor_tab_id);
        }

        Some(())
    }

    pub fn editor_tab_update_layout(
        &self,
        editor_tab_id: &EditorTabId,
        window_origin: Option<Point>,
        rect: Option<Rect>,
    ) -> Option<()> {
        let editor_tabs = self.editor_tabs.get_untracked();
        let editor_tab = editor_tabs.get(editor_tab_id).copied()?;
        editor_tab.update(|editor_tab| {
            if let Some(window_origin) = window_origin {
                editor_tab.window_origin = window_origin;
            }
            if let Some(rect) = rect {
                editor_tab.layout_rect = rect;
            }
        });
        Some(())
    }

    pub fn run_code_action(&self, plugin_id: PluginId, action: CodeActionOrCommand) {
        match action {
            CodeActionOrCommand::Command(command) => {
                self.run_code_lens(
                    &command.command,
                    command.arguments.unwrap_or_default(),
                );
            }
            CodeActionOrCommand::CodeAction(action) => {
                if let Some(edit) = action.edit.as_ref() {
                    self.apply_workspace_edit(edit);
                } else {
                    self.resolve_code_action(plugin_id, action);
                }
            }
        }
    }

    pub fn run_code_lens(&self, command: &str, args: Vec<Value>) {
        self.code_lens.get_untracked().run(command, args);
    }

    /// Resolve a code action and apply its held workspace edit
    fn resolve_code_action(&self, plugin_id: PluginId, action: CodeAction) {
        let main_split = self.clone();
        let send = create_ext_action(self.scope, move |edit| {
            main_split.apply_workspace_edit(&edit);
        });
        self.common
            .proxy
            .code_action_resolve(action, plugin_id, move |result| {
                if let Ok(ProxyResponse::CodeActionResolveResponse { item }) = result
                {
                    if let Some(edit) = item.edit {
                        send(edit);
                    }
                }
            });
    }

    /// Perform a workspace edit, which are from the LSP (such as code actions, or symbol renaming)
    pub fn apply_workspace_edit(&self, edit: &WorkspaceEdit) {
        if let Some(DocumentChanges::Operations(_op)) =
            edit.document_changes.as_ref()
        {
            // TODO
        }

        if let Some(edits) = workspace_edits(edit) {
            for (url, edits) in edits {
                if let Ok(path) = url.to_file_path() {
                    let active_path = self
                        .active_editor
                        .get_untracked()
                        .map(|editor| editor.doc())
                        .map(|doc| doc.content.get_untracked())
                        .and_then(|content| content.path().cloned());
                    let position = if active_path.as_ref() == Some(&path) {
                        None
                    } else {
                        edits
                            .first()
                            .map(|edit| EditorPosition::Position(edit.range.start))
                    };
                    let location = EditorLocation {
                        path,
                        position,
                        scroll_offset: None,
                        ignore_unconfirmed: true,
                        same_editor_tab: false,
                    };
                    self.jump_to_location(location, Some(edits));
                }
            }
        }
    }

    pub fn next_error(&self) {
        let file_diagnostics =
            self.file_diagnostics_items(DiagnosticSeverity::ERROR);
        if file_diagnostics.is_empty() {
            return;
        }
        let active_editor = self.active_editor.get_untracked();
        let active_path = active_editor
            .map(|editor| (editor.doc(), editor.cursor()))
            .and_then(|(doc, cursor)| {
                let offset = cursor.with_untracked(|c| c.offset());
                let (path, position) = (
                    doc.content.get_untracked().path().cloned(),
                    doc.buffer.with_untracked(|b| b.offset_to_position(offset)),
                );
                path.map(|path| (path, offset, position))
            });
        let (path, position) =
            next_in_file_errors_offset(active_path, &file_diagnostics);
        let location = EditorLocation {
            path,
            position: Some(position),
            scroll_offset: None,
            ignore_unconfirmed: false,
            same_editor_tab: false,
        };
        self.jump_to_location(location, None);
    }

    fn file_diagnostics_items(
        &self,
        severity: DiagnosticSeverity,
    ) -> Vec<(PathBuf, Vec<EditorDiagnostic>)> {
        let diagnostics = self.diagnostics.get_untracked();
        diagnostics
            .into_iter()
            .filter_map(|(path, diagnostic)| {
                let span = diagnostic.diagnostics_span.get_untracked();
                if !span.is_empty() {
                    let diags = span
                        .iter()
                        .filter_map(|(iv, diag)| {
                            if diag.severity == Some(severity) {
                                Some(EditorDiagnostic {
                                    range: Some((iv.start, iv.end)),
                                    diagnostic: diag.to_owned(),
                                })
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<EditorDiagnostic>>();
                    if !diags.is_empty() {
                        Some((path, diags))
                    } else {
                        None
                    }
                } else {
                    let diagnostics = diagnostic.diagnostics.get_untracked();
                    let diagnostics: Vec<EditorDiagnostic> = diagnostics
                        .into_iter()
                        .filter(|d| d.severity == Some(severity))
                        .map(|d| EditorDiagnostic {
                            range: None,
                            diagnostic: d,
                        })
                        .collect();
                    if !diagnostics.is_empty() {
                        Some((path, diagnostics))
                    } else {
                        None
                    }
                }
            })
            .sorted_by_key(|(path, _)| path.clone())
            .collect()
    }

    pub fn get_diagnostic_data(&self, path: &Path) -> DiagnosticData {
        if let Some(d) = self.diagnostics.with_untracked(|d| d.get(path).cloned()) {
            d
        } else {
            let diagnostic_data = DiagnosticData {
                expanded: self.scope.create_rw_signal(true),
                diagnostics: self.scope.create_rw_signal(im::Vector::new()),
                diagnostics_span: self
                    .scope
                    .create_rw_signal(SpansBuilder::new(0).build()),
            };
            self.diagnostics.update(|d| {
                d.insert(path.to_path_buf(), diagnostic_data.clone());
            });
            diagnostic_data
        }
    }

    pub fn open_file_changed(&self, path: &Path, content: &FileChanged) {
        tracing::debug!("open_file_changed {:?}", path);
        match content {
            FileChanged::Change(content) => {
                let doc = self.docs.with_untracked(|docs| docs.get(path).cloned());
                let doc = match doc {
                    Some(doc) => doc,
                    None => return,
                };
                doc.handle_file_changed(Rope::from(content));
            }
            FileChanged::Delete => {
                if self.docs.with_untracked(|x| x.get(path).is_none()) {
                    return;
                }
                let Some(editor_id) = self.editors.get_editor_id_by_path(path)
                else {
                    return;
                };
                let id = editor_id.to_raw();
                if let Some(tab_id) = self.editor_tabs.with_untracked(|x| {
                    for (tab_id, tab_data) in x {
                        if tab_data.with_untracked(|x| {
                            x.children.iter().any(|(_, _, child)| child.id() == id)
                        }) {
                            return Some(*tab_id);
                        }
                    }
                    None
                }) {
                    self.editor_tab_close(tab_id);
                }
                self.editors.remove(editor_id);
                self.docs.update(|x| {
                    x.remove(path);
                });
            }
        }
    }

    pub fn set_find_pattern(&self, pattern: Option<String>) {
        if let Some(pattern) = pattern {
            self.find_editor.doc().reload(Rope::from(pattern), true);
        }
        let pattern_len = self
            .find_editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.len());
        self.find_editor
            .cursor()
            .update(|cursor| cursor.set_insert(Selection::region(0, pattern_len)));
    }

    pub fn open_volt_view(&self, id: VoltID) {
        self.get_editor_tab_child(EditorTabChildSource::Volt(id), false, false);
    }

    pub fn open_settings(&self) {
        self.get_editor_tab_child(EditorTabChildSource::Settings, false, false);
    }

    pub fn open_theme_color_settings(&self) {
        self.get_editor_tab_child(
            EditorTabChildSource::ThemeColorSettings,
            false,
            false,
        );
    }

    pub fn open_keymap(&self) {
        self.get_editor_tab_child(EditorTabChildSource::Keymap, false, false);
    }

    pub fn new_file(&self) -> EditorTabChild {
        self.get_editor_tab_child(EditorTabChildSource::NewFileEditor, false, false)
    }

    pub fn save_as(&self, doc: Rc<Doc>, path: PathBuf, action: impl Fn() + 'static) {
        let (buffer_id, doc_content, rev, content) = (
            doc.buffer_id,
            doc.content.get_untracked(),
            doc.rev(),
            doc.buffer.with_untracked(|b| b.to_string()),
        );
        match doc_content {
            DocContent::Scratch { .. } => {
                let send = {
                    let path = path.clone();
                    create_ext_action(self.scope, move |result| {
                        if let Err(err) = result {
                            event!(
                                Level::WARN,
                                "Failed to save as a file: {:?}",
                                err
                            );
                        } else {
                            let syntax = Syntax::init(&path);
                            doc.content.set(DocContent::File {
                                path: path.clone(),
                                read_only: false,
                            });
                            doc.buffer.update(|buffer| {
                                buffer.set_pristine();
                            });
                            doc.set_syntax(syntax);
                            doc.trigger_syntax_change(None);
                            action();
                        }
                    })
                };
                self.common.proxy.save_buffer_as(
                    buffer_id,
                    path,
                    rev,
                    content,
                    true,
                    Box::new(move |result| {
                        send(result);
                    }),
                );
            }
            DocContent::Local => {}
            DocContent::File { .. } => {}
            DocContent::History(_) => {}
        }
    }

    pub fn save_as2(
        &self,
        doc: Rc<Doc>,
        path: PathBuf,
        action: impl Fn() + 'static,
    ) {
        let (buffer_id, doc_content, rev, content) = (
            doc.buffer_id,
            doc.content.get_untracked(),
            doc.rev(),
            doc.buffer.with_untracked(|b| b.to_string()),
        );
        match doc_content {
            DocContent::Scratch { .. } => {
                let send = {
                    let path = path.clone();
                    create_ext_action(self.scope, move |result| {
                        if let Err(err) = result {
                            event!(
                                Level::WARN,
                                "Failed to save as a file: {:?}",
                                err
                            );
                        } else {
                            let syntax = Syntax::init(&path);
                            doc.content.set(DocContent::File {
                                path: path.clone(),
                                read_only: false,
                            });
                            doc.buffer.update(|buffer| {
                                buffer.set_pristine();
                            });
                            doc.set_syntax(syntax);
                            doc.trigger_syntax_change(None);
                            action();
                        }
                    })
                };
                self.common.proxy.save_buffer_as(
                    buffer_id,
                    path,
                    rev,
                    content,
                    true,
                    Box::new(move |result| {
                        send(result);
                    }),
                );
            }
            DocContent::Local => {}
            DocContent::File { .. } => {}
            DocContent::History(_) => {}
        }
    }

    fn get_name_for_new_file(&self) -> String {
        const PREFIX: &str = "Untitled-";

        // Checking just the current scratch_docs rather than all the different document
        // collections seems to be the right thing to do. The user may have genuine 'new N'
        // files tucked away somewhere in their workspace.
        let new_num = self.scratch_docs.with_untracked(|scratch_docs| {
            scratch_docs
                .values()
                .filter_map(|doc| {
                    doc.content.with_untracked(|content| match content {
                        DocContent::Scratch { name, .. } => {
                            // The unwraps are safe because scratch docs are always
                            // titled the same format and the user cannot change the name.
                            let num_part = name.strip_prefix(PREFIX).unwrap();
                            let num = num_part.parse::<i32>().unwrap();
                            Some(num)
                        }
                        _ => None,
                    })
                })
                .max()
                .unwrap_or(0)
                + 1
        });

        format!("{PREFIX}{new_num}")
    }

    pub fn can_jump_location_backward(&self, tracked: bool) -> bool {
        if tracked {
            self.current_location.get() >= 1
        } else {
            self.current_location.get_untracked() >= 1
        }
    }

    pub fn can_jump_location_forward(&self, tracked: bool) -> bool {
        if tracked {
            !(self.locations.with(|l| l.is_empty())
                || self.current_location.get()
                    >= self.locations.with(|l| l.len()) - 1)
        } else {
            !(self.locations.with_untracked(|l| l.is_empty())
                || self.current_location.get_untracked()
                    >= self.locations.with_untracked(|l| l.len()) - 1)
        }
    }

    pub fn save_scratch_doc(&self, doc: Rc<Doc>) {
        let main_split = self.clone();
        save_as(
            FileDialogOptions::new().title("Save File"),
            move |file: Option<FileInfo>| {
                if let Some(mut file) = file {
                    main_split.save_as(
                        doc.clone(),
                        if let Some(path) = file.path.pop() {
                            path
                        } else {
                            tracing::error!("No path");
                            return;
                        },
                        move || {},
                    );
                }
            },
        );
    }

    pub fn save_scratch_doc2(&self, doc: Rc<Doc>) {
        let main_split = self.clone();
        save_as(
            FileDialogOptions::new().title("Save File"),
            move |file: Option<FileInfo>| {
                if let Some(mut file) = file {
                    main_split.save_as2(
                        doc.clone(),
                        if let Some(path) = file.path.pop() {
                            path
                        } else {
                            tracing::error!("No path");
                            return;
                        },
                        move || {},
                    );
                }
            },
        );
    }

    pub fn move_editor_tab_child(
        &self,
        from_tab: EditorTabId,
        to_tab: EditorTabId,
        from_index: usize,
        to_index: usize,
    ) -> Option<()> {
        let from_editor_tab = self
            .editor_tabs
            .with_untracked(|editor_tabs| editor_tabs.get(&from_tab).cloned())?;

        if from_tab == to_tab {
            if from_index == to_index {
                return Some(());
            }

            let to_index = if from_index < to_index {
                to_index - 1
            } else {
                to_index
            };

            from_editor_tab.update(|tab| {
                let child = tab.children.remove(from_index);
                tab.children.insert(to_index, child);
                tab.active = to_index;
            });
        } else {
            let to_editor_tab = self
                .editor_tabs
                .with_untracked(|editor_tabs| editor_tabs.get(&to_tab).cloned())?;

            let (_, _, child) = from_editor_tab
                .try_update(|tab| {
                    let child = tab.children.remove(from_index);
                    tab.active =
                        tab.active.min(tab.children.len().saturating_sub(1));
                    child
                })
                .unwrap();

            self.editor_tab_child_set_parent(&child, to_tab);
            to_editor_tab.update(|tab| {
                tab.children.insert(
                    to_index,
                    (
                        tab.scope.create_rw_signal(to_index),
                        tab.scope.create_rw_signal(Rect::ZERO),
                        child,
                    ),
                );
                tab.active = to_index;
            });
            self.active_editor_tab.set(Some(to_tab));

            if from_editor_tab.with_untracked(|tab| tab.children.is_empty()) {
                self.editor_tab_remove(from_tab);
            }
        }

        Some(())
    }

    fn split_content_set_parent(
        &self,
        content: &SplitContent,
        parent_split_id: SplitId,
    ) -> Option<()> {
        match content {
            SplitContent::EditorTab(editor_tab_id) => {
                let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
                    editor_tabs.get(editor_tab_id).copied()
                })?;
                editor_tab.update(|editor_tab| {
                    editor_tab.split = parent_split_id;
                });
            }
            SplitContent::Split(split_id) => {
                let split = self
                    .splits
                    .with_untracked(|splits| splits.get(split_id).copied())?;
                split.update(|split| {
                    split.parent_split = Some(parent_split_id);
                });
            }
        }
        Some(())
    }

    fn editor_tab_child_set_parent(
        &self,
        child: &EditorTabChild,
        editor_tab_id: EditorTabId,
    ) -> Option<()> {
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self.editors.editor(*editor_id)?;
                editor.editor_tab_id.set(Some(editor_tab_id));
            }
            EditorTabChild::DiffEditor(diff_editor_id) => {
                let diff_editor =
                    self.diff_editors.with_untracked(|diff_editors| {
                        diff_editors.get(diff_editor_id).cloned()
                    })?;
                diff_editor.editor_tab_id.set(editor_tab_id);
                diff_editor
                    .left
                    .diff_editor_id
                    .set(Some((editor_tab_id, *diff_editor_id)));
                diff_editor
                    .right
                    .diff_editor_id
                    .set(Some((editor_tab_id, *diff_editor_id)));
            }
            EditorTabChild::Settings(_) => {}
            EditorTabChild::ThemeColorSettings(_) => {}
            EditorTabChild::Keymap(_) => {}
            EditorTabChild::Volt(_, _) => {}
        }
        Some(())
    }

    pub fn move_editor_tab_child_to_new_split(
        &self,
        from_tab: EditorTabId,
        from_index: usize,
        to_tab: EditorTabId,
        split: SplitMoveDirection,
    ) -> Option<()> {
        let from_editor_tab = self
            .editor_tabs
            .with_untracked(|editor_tabs| editor_tabs.get(&from_tab).cloned())?;
        let to_editor_tab = self
            .editor_tabs
            .with_untracked(|editor_tabs| editor_tabs.get(&to_tab).cloned())?;
        let to_split_id =
            to_editor_tab.with_untracked(|editor_tab| editor_tab.split);
        let to_split = self
            .splits
            .with_untracked(|splits| splits.get(&to_split_id).cloned())?;
        {
            let to_split_direction =
                to_split.with_untracked(|split| split.direction);
            if split.direction() != to_split_direction {
                let children_len =
                    to_split.with_untracked(|split| split.children.len());
                if children_len <= 1 {
                    to_split.update(|to_split| {
                        to_split.direction = split.direction();
                    });
                }
            }
        }

        let to_split_direction = to_split.with_untracked(|split| split.direction);
        if split.direction() == to_split_direction {
            let index =
                to_split.with_untracked(|split| split.editor_tab_index(to_tab))?;
            let index = match split {
                SplitMoveDirection::Up => index,
                SplitMoveDirection::Down => index + 1,
                SplitMoveDirection::Right => index + 1,
                SplitMoveDirection::Left => index,
            };
            let new_editor_tab_id = EditorTabId::next();

            let (_, _, child) = from_editor_tab
                .try_update(|tab| {
                    let child = tab.children.remove(from_index);
                    tab.active =
                        tab.active.min(tab.children.len().saturating_sub(1));
                    child
                })
                .unwrap();

            self.editor_tab_child_set_parent(&child, new_editor_tab_id);

            let cx = self.scope.create_child();
            let new_editor_tab = EditorTabData {
                scope: cx,
                split: to_split_id,
                editor_tab_id: new_editor_tab_id,
                active: 0,
                children: vec![(
                    cx.create_rw_signal(0),
                    cx.create_rw_signal(Rect::ZERO),
                    child,
                )],
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
                locations: cx.create_rw_signal(im::Vector::new()),
                current_location: cx.create_rw_signal(0),
            };
            self.editor_tabs.update(|editor_tabs| {
                editor_tabs.insert(
                    new_editor_tab.editor_tab_id,
                    new_editor_tab.scope.create_rw_signal(new_editor_tab),
                );
            });
            to_split.update(|split| {
                for (size, _) in &split.children {
                    size.set(1.0);
                }
                split.children.insert(
                    index,
                    (
                        split.scope.create_rw_signal(1.0),
                        SplitContent::EditorTab(new_editor_tab_id),
                    ),
                );
            });
            self.active_editor_tab.set(Some(new_editor_tab_id));
        } else {
            let index =
                to_split.with_untracked(|split| split.editor_tab_index(to_tab))?;
            let (size, existing_editor_tab) = to_split
                .try_update(|split| split.children.remove(index))
                .unwrap();

            let new_split_id = SplitId::next();

            if let SplitContent::EditorTab(existing_editor_tab_id) =
                existing_editor_tab
            {
                let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
                    editor_tabs.get(&existing_editor_tab_id).cloned()
                })?;
                editor_tab.update(|editor_tab| {
                    editor_tab.split = new_split_id;
                });
            }

            let new_editor_tab_id = EditorTabId::next();

            let (_, _, child) = from_editor_tab
                .try_update(|tab| {
                    let child = tab.children.remove(from_index);
                    tab.active =
                        tab.active.min(tab.children.len().saturating_sub(1));
                    child
                })
                .unwrap();
            self.editor_tab_child_set_parent(&child, new_editor_tab_id);

            let new_editor_tab = {
                let cx = self.scope.create_child();
                EditorTabData {
                    scope: cx,
                    split: new_split_id,
                    editor_tab_id: new_editor_tab_id,
                    active: 0,
                    children: vec![(
                        cx.create_rw_signal(0),
                        cx.create_rw_signal(Rect::ZERO),
                        child,
                    )],
                    window_origin: Point::ZERO,
                    layout_rect: Rect::ZERO,
                    locations: cx.create_rw_signal(im::Vector::new()),
                    current_location: cx.create_rw_signal(0),
                }
            };
            self.editor_tabs.update(|editor_tabs| {
                editor_tabs.insert(
                    new_editor_tab.editor_tab_id,
                    new_editor_tab.scope.create_rw_signal(new_editor_tab),
                );
            });

            let scope = self.scope.create_child();
            let new_split_children = match split {
                SplitMoveDirection::Up | SplitMoveDirection::Left => {
                    vec![
                        (
                            scope.create_rw_signal(1.0),
                            SplitContent::EditorTab(new_editor_tab_id),
                        ),
                        (scope.create_rw_signal(1.0), existing_editor_tab),
                    ]
                }
                SplitMoveDirection::Down | SplitMoveDirection::Right => {
                    vec![
                        (scope.create_rw_signal(1.0), existing_editor_tab),
                        (
                            scope.create_rw_signal(1.0),
                            SplitContent::EditorTab(new_editor_tab_id),
                        ),
                    ]
                }
            };
            let new_split = SplitData {
                scope,
                split_id: new_split_id,
                parent_split: Some(to_split_id),
                children: new_split_children,
                direction: split.direction(),
                window_origin: Point::ZERO,
                layout_rect: Rect::ZERO,
            };
            self.splits.update(|splits| {
                splits.insert(
                    new_split_id,
                    new_split.scope.create_rw_signal(new_split),
                );
            });
            to_split.update(|split| {
                split.children.insert(
                    index,
                    (
                        split.scope.create_rw_signal(size.get_untracked()),
                        SplitContent::Split(new_split_id),
                    ),
                );
            });
            self.active_editor_tab.set(Some(new_editor_tab_id));
        }

        if from_editor_tab.with_untracked(|tab| tab.children.is_empty()) {
            self.editor_tab_remove(from_tab);
        }

        Some(())
    }

    pub fn export_theme(&self) {
        let child = self.new_file();
        if let EditorTabChild::Editor(id) = child {
            if let Some(editor) = self.editors.editor_untracked(id) {
                let doc = editor.doc();
                doc.reload(
                    Rope::from(self.common.config.get_untracked().export_theme()),
                    true,
                );
            }
        }
    }

    pub fn show_env(&self) {
        let child = self.new_file();
        if let EditorTabChild::Editor(id) = child {
            if let Some(editor) = self.editors.editor_untracked(id) {
                let doc = editor.doc();
                doc.reload(
                    Rope::from(
                        std::env::vars().map(|(k, v)| format!("{k}={v}")).join("\n"),
                    ),
                    true,
                );
            }
        }
    }

    pub fn get_active_editor(&self) -> Option<EditorData> {
        let active_editor_tab = self.active_editor_tab.get()?;
        let editor_tabs = self.editor_tabs;
        let editor_tab = editor_tabs
            .with(|editor_tabs| editor_tabs.get(&active_editor_tab).copied())?;
        let (_, _, child) = editor_tab.with(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        match child {
            EditorTabChild::Editor(editor_id) => self.editors.editor(editor_id),
            _ => None,
        }
    }
}

fn workspace_edits(edit: &WorkspaceEdit) -> Option<HashMap<Url, Vec<TextEdit>>> {
    if let Some(changes) = edit.changes.as_ref() {
        return Some(changes.clone());
    }

    let changes = edit.document_changes.as_ref()?;
    let edits = match changes {
        DocumentChanges::Edits(edits) => edits
            .iter()
            .map(|e| {
                (
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
        DocumentChanges::Operations(ops) => ops
            .iter()
            .filter_map(|o| match o {
                DocumentChangeOperation::Op(_op) => None,
                DocumentChangeOperation::Edit(e) => Some((
                    e.text_document.uri.clone(),
                    e.edits
                        .iter()
                        .map(|e| match e {
                            OneOf::Left(e) => e.clone(),
                            OneOf::Right(e) => e.text_edit.clone(),
                        })
                        .collect(),
                )),
            })
            .collect::<HashMap<Url, Vec<TextEdit>>>(),
    };
    Some(edits)
}

fn next_in_file_errors_offset(
    active_path: Option<(PathBuf, usize, Position)>,
    file_diagnostics: &[(PathBuf, Vec<EditorDiagnostic>)],
) -> (PathBuf, EditorPosition) {
    if let Some((active_path, offset, position)) = active_path {
        for (current_path, diagnostics) in file_diagnostics {
            if &active_path == current_path {
                for diagnostic in diagnostics {
                    if let Some((start, _)) = diagnostic.range {
                        if start > offset {
                            return (
                                (*current_path).clone(),
                                EditorPosition::Offset(start),
                            );
                        }
                    }

                    if diagnostic.diagnostic.range.start.line > position.line
                        || (diagnostic.diagnostic.range.start.line == position.line
                            && diagnostic.diagnostic.range.start.character
                                > position.character)
                    {
                        return (
                            (*current_path).clone(),
                            EditorPosition::Position(
                                diagnostic.diagnostic.range.start,
                            ),
                        );
                    }
                }
            }
            if current_path > &active_path {
                if let Some((start, _)) = diagnostics[0].range {
                    return ((*current_path).clone(), EditorPosition::Offset(start));
                }
                return (
                    (*current_path).clone(),
                    if let Some((start, _)) = diagnostics[0].range {
                        EditorPosition::Offset(start)
                    } else {
                        EditorPosition::Position(
                            diagnostics[0].diagnostic.range.start,
                        )
                    },
                );
            }
        }
    }

    (
        file_diagnostics[0].0.clone(),
        if let Some((start, _)) = file_diagnostics[0].1[0].range {
            EditorPosition::Offset(start)
        } else {
            EditorPosition::Position(file_diagnostics[0].1[0].diagnostic.range.start)
        },
    )
}

#[derive(Clone, Copy, Debug)]
pub enum TabCloseKind {
    CloseOther,
    CloseToLeft,
    CloseToRight,
}
