use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use floem::{
    app::AppContext,
    ext_event::create_ext_action,
    glazier::KeyEvent,
    reactive::{create_rw_signal, ReadSignal, RwSignal, UntrackedGettableSignal},
};
use lapce_core::register::Register;
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};
use lapce_xi_rope::Rope;
use serde::{Deserialize, Serialize};

use crate::{
    config::LapceConfig,
    doc::{DocContent, Document},
    editor::EditorData,
    editor_tab::{EditorTabChild, EditorTabData},
    id::{EditorId, EditorTabId, SplitId},
    keypress::{KeyPressData, KeyPressFocus},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitDirection {
    Vertical,
    Horizontal,
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
}

#[derive(Clone)]
pub struct SplitData {
    pub parent_split: Option<SplitId>,
    pub split_id: SplitId,
    pub children: Vec<SplitContent>,
    pub direction: SplitDirection,
}

#[derive(Clone)]
pub struct MainSplitData {
    pub root_split: SplitId,
    pub active_editor_tab: RwSignal<Option<EditorTabId>>,
    pub splits: RwSignal<im::HashMap<SplitId, RwSignal<SplitData>>>,
    pub editor_tabs: RwSignal<im::HashMap<EditorTabId, RwSignal<EditorTabData>>>,
    pub editors: RwSignal<im::HashMap<EditorId, RwSignal<EditorData>>>,
    pub docs: RwSignal<im::HashMap<PathBuf, RwSignal<Document>>>,
    pub proxy_rpc: ProxyRpcHandler,
    register: RwSignal<Register>,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl MainSplitData {
    pub fn new(
        cx: AppContext,
        proxy_rpc: ProxyRpcHandler,
        register: RwSignal<Register>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let root_split = SplitId::next();
        let root_split_data = SplitData {
            parent_split: None,
            split_id: root_split,
            children: Vec::new(),
            direction: SplitDirection::Horizontal,
        };

        let mut splits = im::HashMap::new();
        splits.insert(root_split, create_rw_signal(cx.scope, root_split_data));
        let splits = create_rw_signal(cx.scope, splits);

        let active_editor_tab = create_rw_signal(cx.scope, None);
        let editor_tabs = create_rw_signal(cx.scope, im::HashMap::new());
        let editors = create_rw_signal(cx.scope, im::HashMap::new());
        let docs = create_rw_signal(cx.scope, im::HashMap::new());

        Self {
            root_split,
            splits,
            active_editor_tab,
            editor_tabs,
            editors,
            docs,
            proxy_rpc,
            register,
            config,
        }
    }

    pub fn key_down(
        &self,
        cx: AppContext,
        key_event: &KeyEvent,
        keypress: &mut KeyPressData,
    ) -> Option<()> {
        let active_editor_tab = self.active_editor_tab.get_untracked()?;
        let editor_tab = self.editor_tabs.with_untracked(|editor_tabs| {
            editor_tabs.get(&active_editor_tab).copied()
        })?;
        let child = editor_tab.with_untracked(|editor_tab| {
            editor_tab.children.get(editor_tab.active).cloned()
        })?;
        match child {
            EditorTabChild::Editor(editor_id) => {
                let editor = self
                    .editors
                    .with_untracked(|editors| editors.get(&editor_id).copied())?;
                editor.with_untracked(|editor| {
                    keypress.key_down(cx, key_event, editor);
                });
            }
        }
        Some(())
    }

    pub fn open_file(&self, cx: AppContext, path: PathBuf) {
        let doc = self.docs.with_untracked(|docs| docs.get(&path).cloned());
        let doc = if let Some(doc) = doc {
            doc
        } else {
            let doc = Document::new(path.clone(), self.config);
            let buffer_id = doc.buffer_id;
            let doc = create_rw_signal(cx.scope, doc);
            self.docs.update(|docs| {
                docs.insert(path.clone(), doc);
            });

            let set_doc = doc.write_only();
            let send = create_ext_action(cx, move |content| {
                set_doc.update(|doc| {
                    doc.init_content(content);
                });
            });

            self.proxy_rpc
                .new_buffer(buffer_id, path.clone(), move |result| {
                    if let Ok(ProxyResponse::NewBufferResponse { content }) = result
                    {
                        send(Rope::from(content))
                    }
                });

            doc
        };
        self.get_editor_or_new(cx, doc, &path);
    }

    fn get_editor_or_new(
        &self,
        cx: AppContext,
        doc: RwSignal<Document>,
        path: &Path,
    ) -> RwSignal<EditorData> {
        let active_editor_tab_id = self.active_editor_tab.get_untracked();
        let editor_tabs = self.editor_tabs.get_untracked();
        let editors = self.editors.get_untracked();
        let splits = self.splits.get_untracked();

        let active_editor_tab = active_editor_tab_id
            .and_then(|id| editor_tabs.get(&id))
            .cloned();

        // first check if the file exists in active editor tab
        if let Some(editor_tab) = active_editor_tab {
            if let Some(editor) = editor_tab
                .with_untracked(|editor_tab| editor_tab.get_editor(&editors, path))
            {
                return editor;
            }
        }

        // check file exists in non active editor tabs
        for (editor_tab_id, editor_tab) in &editor_tabs {
            if Some(*editor_tab_id) != active_editor_tab_id {
                if let Some(editor) = editor_tab.with_untracked(|editor_tab| {
                    editor_tab.get_editor(&editors, path)
                }) {
                    self.active_editor_tab.set(Some(*editor_tab_id));
                    return editor;
                }
            }
        }

        // create the new editor
        let editor_id = EditorId::next();
        let editor = EditorData::new(cx, doc, self.register, self.config);
        let editor = create_rw_signal(cx.scope, editor);
        self.editors.update(|editors| {
            editors.insert(editor_id, editor);
        });

        if editor_tabs.is_empty() {
            // the main split doens't have anything
            let editor_tab_id = EditorTabId::next();
            let editor_tab = EditorTabData {
                split: self.root_split,
                active: 0,
                editor_tab_id,
                children: vec![EditorTabChild::Editor(editor_id)],
            };
            let editor_tab = create_rw_signal(cx.scope, editor_tab);
            self.editor_tabs.update(|editor_tabs| {
                editor_tabs.insert(editor_tab_id, editor_tab);
            });
            let root_split = splits.get(&self.root_split).unwrap();
            root_split.update(|root_split| {
                root_split.children = vec![SplitContent::EditorTab(editor_tab_id)];
            });
            self.active_editor_tab.set(Some(editor_tab_id));
        } else {
            let editor_tab = if let Some(editor_tab) = active_editor_tab {
                editor_tab
            } else {
                let (editor_tab_id, editor_tab) = editor_tabs.iter().next().unwrap();
                self.active_editor_tab.set(Some(*editor_tab_id));
                *editor_tab
            };

            println!("add editor tab child");

            editor_tab.update(|editor_tab| {
                let active = editor_tab
                    .active
                    .min(editor_tab.children.len().saturating_sub(1));
                editor_tab
                    .children
                    .insert(active + 1, EditorTabChild::Editor(editor_id));
                editor_tab.active = active + 1;
            });
        }

        editor
    }
}
