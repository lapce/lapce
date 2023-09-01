use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use floem::{
    ext_event::{create_ext_action, create_signal_from_channel},
    keyboard::ModifiersState,
    reactive::{use_context, ReadSignal, RwSignal, Scope},
};
use itertools::Itertools;
use lapce_core::{
    buffer::rope_text::RopeText, command::FocusCommand, language::LapceLanguage,
    mode::Mode, movement::Movement, selection::Selection, syntax::Syntax,
};
use lapce_rpc::proxy::ProxyResponse;
use lapce_xi_rope::Rope;
use lsp_types::DocumentSymbolResponse;
use nucleo::Utf32Str;
use strum::{EnumMessage, IntoEnumIterator};

use self::{
    item::{PaletteItem, PaletteItemContent},
    kind::PaletteKind,
};
use crate::{
    command::{
        CommandExecuted, CommandKind, InternalCommand, LapceCommand, WindowCommand,
    },
    db::LapceDb,
    debug::{run_configs, RunDebugMode},
    editor::{
        location::{EditorLocation, EditorPosition},
        EditorData,
    },
    id::EditorId,
    keypress::{condition::Condition, KeyPressData, KeyPressFocus},
    main_split::MainSplitData,
    proxy::path_from_url,
    source_control::SourceControlData,
    window_tab::{CommonData, Focus},
    workspace::{LapceWorkspace, LapceWorkspaceType, SshHost},
};

pub mod item;
pub mod kind;

#[derive(Clone, PartialEq, Eq)]
pub enum PaletteStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone, Debug)]
pub struct PaletteInput {
    pub input: String,
    pub kind: PaletteKind,
}

impl PaletteInput {
    /// Update the current input in the palette, and the kind of palette it is
    pub fn update_input(&mut self, input: String, kind: PaletteKind) {
        self.kind = kind.get_palette_kind(&input);
        self.input = self.kind.get_input(&input).to_string();
    }
}

#[derive(Clone)]
pub struct PaletteData {
    run_id_counter: Arc<AtomicU64>,
    pub run_id: RwSignal<u64>,
    pub workspace: Arc<LapceWorkspace>,
    pub status: RwSignal<PaletteStatus>,
    pub index: RwSignal<usize>,
    pub items: RwSignal<im::Vector<PaletteItem>>,
    pub filtered_items: ReadSignal<im::Vector<PaletteItem>>,
    pub input: RwSignal<PaletteInput>,
    kind: RwSignal<PaletteKind>,
    pub input_editor: EditorData,
    pub preview_editor: Rc<EditorData>,
    pub has_preview: RwSignal<bool>,
    pub keypress: ReadSignal<KeyPressData>,
    /// Listened on for which entry in the palette has been clicked
    pub clicked_index: RwSignal<Option<usize>>,
    pub executed_commands: Rc<RefCell<HashMap<String, Instant>>>,
    pub executed_run_configs: Rc<RefCell<HashMap<(RunDebugMode, String), Instant>>>,
    pub main_split: MainSplitData,
    pub references: RwSignal<Vec<EditorLocation>>,
    pub source_control: SourceControlData,
    pub common: Rc<CommonData>,
}

impl PaletteData {
    pub fn new(
        cx: Scope,
        workspace: Arc<LapceWorkspace>,
        main_split: MainSplitData,
        keypress: ReadSignal<KeyPressData>,
        source_control: SourceControlData,
        common: Rc<CommonData>,
    ) -> Self {
        let status = cx.create_rw_signal(PaletteStatus::Inactive);
        let items = cx.create_rw_signal(im::Vector::new());
        let index = cx.create_rw_signal(0);
        let references = cx.create_rw_signal(Vec::new());
        let input = cx.create_rw_signal(PaletteInput {
            input: "".to_string(),
            kind: PaletteKind::File,
        });
        let kind = cx.create_rw_signal(PaletteKind::File);
        let input_editor =
            EditorData::new_local(cx, EditorId::next(), common.clone());
        let preview_editor =
            EditorData::new_local(cx, EditorId::next(), common.clone());
        let preview_editor = Rc::new(preview_editor);
        let has_preview = cx.create_rw_signal(false);
        let run_id = cx.create_rw_signal(0);
        let run_id_counter = Arc::new(AtomicU64::new(0));

        let (run_tx, run_rx) = crossbeam_channel::unbounded();
        {
            let run_id = run_id.read_only();
            let input = input.read_only();
            let items = items.read_only();
            let tx = run_tx;
            {
                let tx = tx.clone();
                // this effect only monitors items change
                cx.create_effect(move |_| {
                    let items = items.get();
                    let input = input.get_untracked();
                    let run_id = run_id.get_untracked();
                    let _ = tx.send((run_id, input.input, items));
                });
            }
            // this effect only monitors input change
            cx.create_effect(move |last_kind| {
                let input = input.get();
                let kind = input.kind;
                if last_kind != Some(kind) {
                    return kind;
                }
                let items = items.get_untracked();
                let run_id = run_id.get_untracked();
                let _ = tx.send((run_id, input.input, items));
                kind
            });
        }
        let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
        {
            let run_id = run_id_counter.clone();
            std::thread::spawn(move || {
                Self::update_process(run_id, run_rx, resp_tx);
            });
        }
        let (filtered_items, set_filtered_items) =
            cx.create_signal(im::Vector::new());
        {
            let resp = create_signal_from_channel(resp_rx);
            let run_id = run_id.read_only();
            let input = input.read_only();
            cx.create_effect(move |_| {
                if let Some((filter_run_id, filter_input, new_items)) = resp.get() {
                    if run_id.get_untracked() == filter_run_id
                        && input.get_untracked().input == filter_input
                    {
                        set_filtered_items.set(new_items);
                        index.set(0);
                    }
                }
            });
        }

        let clicked_index = cx.create_rw_signal(Option::<usize>::None);

        let palette = Self {
            run_id_counter,
            main_split,
            run_id,
            workspace,
            status,
            index,
            items,
            filtered_items,
            input_editor,
            preview_editor,
            has_preview,
            input,
            kind,
            keypress,
            clicked_index,
            executed_commands: Rc::new(RefCell::new(HashMap::new())),
            executed_run_configs: Rc::new(RefCell::new(HashMap::new())),
            references,
            source_control,
            common,
        };

        {
            let palette = palette.clone();
            let clicked_index = clicked_index.read_only();
            let index = index.write_only();
            cx.create_effect(move |_| {
                if let Some(clicked_index) = clicked_index.get() {
                    index.set(clicked_index);
                    palette.select();
                }
            });
        }

        {
            let palette = palette.clone();
            let doc = palette.input_editor.view.doc.get_untracked();
            let input = palette.input;
            let status = palette.status.read_only();
            let preset_kind = palette.kind.read_only();
            // Monitors when the palette's input changes, so that it can update the stored input
            // and kind of palette.
            cx.create_effect(move |last_input| {
                // TODO(minor, perf): this could have perf issues if the user accidentally pasted a huge amount of text into the palette.
                let new_input = doc.buffer.with(|buffer| buffer.to_string());

                let status = status.get_untracked();
                if status == PaletteStatus::Inactive {
                    // If the status is inactive, we set the input to None,
                    // so that when we actually run the palette, the input
                    // can be compared with this None.
                    return None;
                }

                let last_input = last_input.flatten();

                // If the input is not equivalent to the current input, or not initialized, then we
                // need to update the information about the palette.
                let changed = last_input.as_deref() != Some(new_input.as_str());

                if changed {
                    let new_kind = input
                        .try_update(|input| {
                            let kind = input.kind;
                            input.update_input(
                                new_input.clone(),
                                preset_kind.get_untracked(),
                            );
                            if last_input.is_none() || kind != input.kind {
                                Some(input.kind)
                            } else {
                                None
                            }
                        })
                        .unwrap();
                    if let Some(new_kind) = new_kind {
                        palette.run_inner(new_kind);
                    } else if input
                        .with_untracked(|i| i.kind == PaletteKind::WorkspaceSymbol)
                    {
                        palette.run_inner(PaletteKind::WorkspaceSymbol);
                    }
                }
                Some(new_input)
            });
        }

        {
            let palette = palette.clone();
            cx.create_effect(move |_| {
                let _ = palette.index.get();
                palette.preview(cx);
            });
        }

        {
            let palette = palette.clone();
            cx.create_effect(move |_| {
                let focus = palette.common.focus.get();
                if focus != Focus::Palette
                    && palette.status.get_untracked() != PaletteStatus::Inactive
                {
                    palette.cancel();
                }
            });
        }

        palette
    }

    /// Start and focus the palette for the given kind.
    pub fn run(&self, kind: PaletteKind) {
        self.common.focus.set(Focus::Palette);
        self.status.set(PaletteStatus::Started);
        let symbol = kind.symbol();
        self.kind.set(kind);
        // Refresh the palette input with only the symbol prefix, losing old content.
        self.input_editor
            .view
            .doc
            .get_untracked()
            .reload(Rope::from(symbol), true);
        self.input_editor
            .cursor
            .update(|cursor| cursor.set_insert(Selection::caret(symbol.len())));
    }

    /// Execute the internal behavior of the palette for the given kind. This ignores updating and
    /// focusing the palette input.
    fn run_inner(&self, kind: PaletteKind) {
        self.has_preview.set(false);

        let run_id = self.run_id_counter.fetch_add(1, Ordering::Relaxed) + 1;
        self.run_id.set(run_id);

        match kind {
            PaletteKind::PaletteHelp => self.get_palette_help(),
            PaletteKind::File => {
                self.get_files();
            }
            PaletteKind::Line => {
                self.get_lines();
            }
            PaletteKind::Command => {
                self.get_commands();
            }
            PaletteKind::Workspace => {
                self.get_workspaces();
            }
            PaletteKind::Reference => {
                self.get_references();
            }
            PaletteKind::DocumentSymbol => {
                self.get_document_symbols();
            }
            PaletteKind::WorkspaceSymbol => {
                self.get_workspace_symbols();
            }
            PaletteKind::SshHost => {
                self.get_ssh_hosts();
            }
            PaletteKind::RunAndDebug => {
                self.get_run_configs();
            }
            PaletteKind::ColorTheme => {
                self.get_color_themes();
                self.preselect_matching(
                    &self.common.config.get_untracked().color_theme.name,
                );
            }
            PaletteKind::IconTheme => {
                self.get_icon_themes();
                self.preselect_matching(
                    &self.common.config.get_untracked().icon_theme.name,
                );
            }
            PaletteKind::Language => {
                self.get_languages();
                if let Some(editor) = self.main_split.active_editor.get_untracked() {
                    let doc = editor.view.doc.get_untracked();
                    let language = doc
                        .syntax
                        .with_untracked(|syntax| syntax.language.to_string());
                    self.preselect_matching(language.as_str());
                }
            }
            PaletteKind::SCMReferences => {
                self.get_scm_references();
            }
        }
    }

    /// Initialize the palette with a list of the available palette kinds.
    fn get_palette_help(&self) {
        let items = PaletteKind::iter()
            .filter_map(|kind| {
                // Don't include PaletteHelp as the user is already here.
                (kind != PaletteKind::PaletteHelp)
                    .then(|| {
                        let symbol = kind.symbol();

                        // Only include palette kinds accessible by typing a prefix into the
                        // palette.
                        (kind == PaletteKind::File || !symbol.is_empty())
                            .then_some(kind)
                    })
                    .flatten()
            })
            .filter_map(|kind| kind.command().map(|cmd| (kind, cmd)))
            .map(|(kind, cmd)| {
                let description = kind.symbol().to_string()
                    + " "
                    + cmd.get_message().unwrap_or("");

                PaletteItem {
                    content: PaletteItemContent::PaletteHelp { cmd },
                    filter_text: description,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();

        self.items.set(items);
    }

    /// Initialize the palette with the files in the current workspace.
    fn get_files(&self) {
        let workspace = self.workspace.clone();
        let set_items = self.items.write_only();
        let send =
            create_ext_action(self.common.scope, move |items: Vec<PathBuf>| {
                let items = items
                    .into_iter()
                    .map(|path| {
                        let full_path = path.clone();
                        // Strip the workspace prefix off the path, to avoid clutter
                        let path =
                            if let Some(workspace_path) = workspace.path.as_ref() {
                                path.strip_prefix(workspace_path)
                                    .unwrap_or(&full_path)
                                    .to_path_buf()
                            } else {
                                path
                            };
                        let filter_text = path.to_str().unwrap_or("").to_string();
                        PaletteItem {
                            content: PaletteItemContent::File { path, full_path },
                            filter_text,
                            score: 0,
                            indices: Vec::new(),
                        }
                    })
                    .collect::<im::Vector<_>>();
                set_items.set(items);
            });
        self.common.proxy.get_files(move |result| {
            if let Ok(ProxyResponse::GetFilesResponse { items }) = result {
                send(items);
            }
        });
    }

    /// Initialize the palette with the lines in the current document.
    fn get_lines(&self) {
        let editor = self.main_split.active_editor.get_untracked();
        let doc = match editor {
            Some(editor) => editor.view.doc.get_untracked(),
            None => {
                return;
            }
        };

        let buffer = doc.buffer.get_untracked();
        let last_line_number = buffer.last_line() + 1;
        let last_line_number_len = last_line_number.to_string().len();
        let items = buffer
            .text()
            .lines(0..buffer.len())
            .enumerate()
            .map(|(i, l)| {
                let line_number = i + 1;
                let text = format!(
                    "{}{} {}",
                    line_number,
                    vec![" "; last_line_number_len - line_number.to_string().len()]
                        .join(""),
                    l
                );
                PaletteItem {
                    content: PaletteItemContent::Line {
                        line: i,
                        content: text.clone(),
                    },
                    filter_text: text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();
        self.items.set(items);
    }

    fn get_commands(&self) {
        const EXCLUDED_ITEMS: &[&str] = &["palette.command"];

        let items = self.keypress.with_untracked(|keypress| {
            // Get all the commands we've executed, and sort them by how recently they were
            // executed. Ignore commands without descriptions.
            let mut items: im::Vector<PaletteItem> = self
                .executed_commands
                .borrow()
                .iter()
                .sorted_by_key(|(_, i)| *i)
                .rev()
                .filter_map(|(key, _)| {
                    keypress.commands.get(key).and_then(|c| {
                        c.kind.desc().as_ref().map(|m| PaletteItem {
                            content: PaletteItemContent::Command { cmd: c.clone() },
                            filter_text: m.to_string(),
                            score: 0,
                            indices: vec![],
                        })
                    })
                })
                .collect();
            // Add all the rest of the commands, ignoring palette commands (because we're in it)
            // and commands that are sorted earlier due to being executed.
            items.extend(keypress.commands.iter().filter_map(|(_, c)| {
                if EXCLUDED_ITEMS.contains(&c.kind.str()) {
                    return None;
                }

                if self.executed_commands.borrow().contains_key(c.kind.str()) {
                    return None;
                }

                c.kind.desc().as_ref().map(|m| PaletteItem {
                    content: PaletteItemContent::Command { cmd: c.clone() },
                    filter_text: m.to_string(),
                    score: 0,
                    indices: vec![],
                })
            }));

            items
        });

        self.items.set(items);
    }

    /// Initialize the palette with all the available workspaces, local and remote.
    fn get_workspaces(&self) {
        let db: Arc<LapceDb> = use_context().unwrap();
        let workspaces = db.recent_workspaces().unwrap_or_default();

        let items = workspaces
            .into_iter()
            .filter_map(|w| {
                let text = w.path.as_ref()?.to_str()?.to_string();
                let filter_text = match &w.kind {
                    LapceWorkspaceType::Local => text,
                    LapceWorkspaceType::RemoteSSH(ssh) => {
                        format!("[{ssh}] {text}")
                    }
                    #[cfg(windows)]
                    LapceWorkspaceType::RemoteWSL => {
                        format!("[wsl] {text}")
                    }
                };
                Some(PaletteItem {
                    content: PaletteItemContent::Workspace { workspace: w },
                    filter_text,
                    score: 0,
                    indices: vec![],
                })
            })
            .collect();

        self.items.set(items);
    }

    /// Initialize the list of references in the file, from the current editor location.
    fn get_references(&self) {
        let items = self
            .references
            .get_untracked()
            .into_iter()
            .map(|l| {
                let full_path = l.path.clone();
                let mut path = l.path.clone();
                if let Some(workspace_path) = self.workspace.path.as_ref() {
                    path = path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&full_path)
                        .to_path_buf();
                }
                let filter_text = path.to_str().unwrap_or("").to_string();
                PaletteItem {
                    content: PaletteItemContent::Reference { path, location: l },
                    filter_text,
                    score: 0,
                    indices: vec![],
                }
            })
            .collect();

        self.items.set(items);
    }

    fn get_document_symbols(&self) {
        let editor = self.main_split.active_editor.get_untracked();
        let doc = match editor {
            Some(editor) => editor.view.doc,
            None => {
                self.items.update(|items| items.clear());
                return;
            }
        };
        let path = doc
            .get_untracked()
            .content
            .with_untracked(|content| content.path().cloned());
        let path = match path {
            Some(path) => path,
            None => {
                self.items.update(|items| items.clear());
                return;
            }
        };

        let set_items = self.items.write_only();
        let send = create_ext_action(self.common.scope, move |result| {
            if let Ok(ProxyResponse::GetDocumentSymbols { resp }) = result {
                let items: im::Vector<PaletteItem> = match resp {
                    DocumentSymbolResponse::Flat(symbols) => symbols
                        .iter()
                        .map(|s| {
                            let mut filter_text = s.name.clone();
                            if let Some(container_name) = s.container_name.as_ref() {
                                filter_text += container_name;
                            }
                            PaletteItem {
                                content: PaletteItemContent::DocumentSymbol {
                                    kind: s.kind,
                                    name: s.name.clone(),
                                    range: s.location.range,
                                    container_name: s.container_name.clone(),
                                },
                                filter_text,
                                score: 0,
                                indices: Vec::new(),
                            }
                        })
                        .collect(),
                    DocumentSymbolResponse::Nested(symbols) => symbols
                        .iter()
                        .map(|s| PaletteItem {
                            content: PaletteItemContent::DocumentSymbol {
                                kind: s.kind,
                                name: s.name.clone(),
                                range: s.range,
                                container_name: None,
                            },
                            filter_text: s.name.clone(),
                            score: 0,
                            indices: Vec::new(),
                        })
                        .collect(),
                };
                set_items.set(items);
            } else {
                set_items.update(|items| items.clear());
            }
        });

        self.common.proxy.get_document_symbols(path, move |result| {
            send(result);
        });
    }

    fn get_workspace_symbols(&self) {
        let input = self.input.get_untracked().input;

        let set_items = self.items.write_only();
        let send = create_ext_action(self.common.scope, move |result| {
            if let Ok(ProxyResponse::GetWorkspaceSymbols { symbols }) = result {
                let items: im::Vector<PaletteItem> = symbols
                    .iter()
                    .map(|s| {
                        // TODO: Should we be using filter text?
                        let mut filter_text = s.name.clone();
                        if let Some(container_name) = s.container_name.as_ref() {
                            filter_text += container_name;
                        }
                        PaletteItem {
                            content: PaletteItemContent::WorkspaceSymbol {
                                kind: s.kind,
                                name: s.name.clone(),
                                location: EditorLocation {
                                    path: path_from_url(&s.location.uri),
                                    position: Some(EditorPosition::Position(
                                        s.location.range.start,
                                    )),
                                    scroll_offset: None,
                                    ignore_unconfirmed: false,
                                    same_editor_tab: false,
                                },
                                container_name: s.container_name.clone(),
                            },
                            filter_text,
                            score: 0,
                            indices: Vec::new(),
                        }
                    })
                    .collect();
                set_items.set(items);
            } else {
                set_items.update(|items| items.clear());
            }
        });

        self.common
            .proxy
            .get_workspace_symbols(input, move |result| {
                send(result);
            });
    }

    fn get_ssh_hosts(&self) {
        let db: Arc<LapceDb> = use_context().unwrap();
        let workspaces = db.recent_workspaces().unwrap_or_default();
        let mut hosts = HashSet::new();
        for workspace in workspaces.iter() {
            if let LapceWorkspaceType::RemoteSSH(ssh) = &workspace.kind {
                hosts.insert(ssh.clone());
            }
        }

        let items = hosts
            .iter()
            .map(|ssh| PaletteItem {
                content: PaletteItemContent::SshHost { host: ssh.clone() },
                filter_text: ssh.to_string(),
                score: 0,
                indices: vec![],
            })
            .collect();
        self.items.set(items);
    }

    fn get_run_configs(&self) {
        let configs = run_configs(self.common.workspace.path.as_deref());
        if configs.is_none() {
            if let Some(path) = self.workspace.path.as_ref() {
                let path = path.join(".lapce").join("run.toml");
                self.common
                    .internal_command
                    .send(InternalCommand::OpenFile { path });
            }
        }
        let executed_run_configs = self.executed_run_configs.borrow();

        let mut items = Vec::new();
        if let Some(configs) = configs.as_ref() {
            for config in &configs.configs {
                items.push((
                    executed_run_configs
                        .get(&(RunDebugMode::Run, config.name.clone())),
                    PaletteItem {
                        content: PaletteItemContent::RunAndDebug {
                            mode: RunDebugMode::Run,
                            config: config.clone(),
                        },
                        filter_text: format!(
                            "Run {} {} {}",
                            config.name,
                            config.program,
                            config.args.join(" ")
                        ),
                        score: 0,
                        indices: vec![],
                    },
                ));
                items.push((
                    executed_run_configs
                        .get(&(RunDebugMode::Debug, config.name.clone())),
                    PaletteItem {
                        content: PaletteItemContent::RunAndDebug {
                            mode: RunDebugMode::Debug,
                            config: config.clone(),
                        },
                        filter_text: format!(
                            "Debug {} {} {}",
                            config.name,
                            config.program,
                            config.args.join(" ")
                        ),
                        score: 0,
                        indices: vec![],
                    },
                ));
            }
        }

        items.sort_by_key(|(executed, _item)| std::cmp::Reverse(executed.copied()));
        self.items
            .set(items.into_iter().map(|(_, item)| item).collect());
    }

    fn get_color_themes(&self) {
        let config = self.common.config.get_untracked();
        let items = config
            .color_theme_list()
            .iter()
            .map(|name| PaletteItem {
                content: PaletteItemContent::ColorTheme { name: name.clone() },
                filter_text: name.clone(),
                score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.items.set(items);
    }

    fn get_icon_themes(&self) {
        let config = self.common.config.get_untracked();
        let items = config
            .icon_theme_list()
            .iter()
            .map(|name| PaletteItem {
                content: PaletteItemContent::IconTheme { name: name.clone() },
                filter_text: name.clone(),
                score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.items.set(items);
    }

    fn get_languages(&self) {
        let langs = LapceLanguage::languages();
        let items = langs
            .iter()
            .map(|lang| PaletteItem {
                content: PaletteItemContent::Language {
                    name: lang.to_owned().to_owned(),
                },
                filter_text: lang.to_owned().to_owned(),
                score: 0,
                indices: Vec::new(),
            })
            .collect();
        self.items.set(items);
    }

    fn get_scm_references(&self) {
        let branches = self.source_control.branches.get_untracked();
        let tags = self.source_control.tags.get_untracked();
        let mut items: im::Vector<PaletteItem> = im::Vector::new();
        for refs in branches.into_iter() {
            items.push_back(PaletteItem {
                content: PaletteItemContent::SCMReference {
                    name: refs.to_owned(),
                },
                filter_text: refs.to_owned(),
                score: 0,
                indices: Vec::new(),
            });
        }
        for refs in tags.into_iter() {
            items.push_back(PaletteItem {
                content: PaletteItemContent::SCMReference {
                    name: refs.to_owned(),
                },
                filter_text: refs.to_owned(),
                score: 0,
                indices: Vec::new(),
            });
        }
        self.items.set(items);
    }

    fn preselect_matching(&self, matching: &str) {
        let Some((idx, _)) = self
            .items
            .get_untracked()
            .iter()
            .find_position(|item| item.filter_text == matching)
        else {
            return;
        };

        self.index.set(idx);
    }

    fn select(&self) {
        let index = self.index.get_untracked();
        let items = self.filtered_items.get_untracked();
        self.close();
        if let Some(item) = items.get(index) {
            match &item.content {
                PaletteItemContent::PaletteHelp { cmd } => {
                    let cmd = LapceCommand {
                        kind: CommandKind::Workbench(cmd.clone()),
                        data: None,
                    };

                    self.common.lapce_command.send(cmd);
                }
                PaletteItemContent::File { full_path, .. } => {
                    self.common
                        .internal_command
                        .send(InternalCommand::OpenFile {
                            path: full_path.to_owned(),
                        });
                }
                PaletteItemContent::Line { line, .. } => {
                    let editor = self.main_split.active_editor.get_untracked();
                    let doc = match editor {
                        Some(editor) => editor.view.doc,
                        None => {
                            return;
                        }
                    };
                    let path = doc
                        .get_untracked()
                        .content
                        .with_untracked(|content| content.path().cloned());
                    let path = match path {
                        Some(path) => path,
                        None => return,
                    };
                    self.common.internal_command.send(
                        InternalCommand::JumpToLocation {
                            location: EditorLocation {
                                path,
                                position: Some(EditorPosition::Line(*line)),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            },
                        },
                    );
                }
                PaletteItemContent::Command { cmd } => {
                    self.common.lapce_command.send(cmd.clone());
                }
                PaletteItemContent::Workspace { workspace } => {
                    self.common
                        .window_command
                        .send(WindowCommand::SetWorkspace {
                            workspace: workspace.clone(),
                        });
                }
                PaletteItemContent::Reference { location, .. } => {
                    self.common.internal_command.send(
                        InternalCommand::JumpToLocation {
                            location: location.clone(),
                        },
                    );
                }
                PaletteItemContent::SshHost { host } => {
                    self.common
                        .window_command
                        .send(WindowCommand::SetWorkspace {
                            workspace: LapceWorkspace {
                                kind: LapceWorkspaceType::RemoteSSH(host.clone()),
                                path: None,
                                last_open: 0,
                            },
                        });
                }
                PaletteItemContent::DocumentSymbol { range, .. } => {
                    let editor = self.main_split.active_editor.get_untracked();
                    let doc = match editor {
                        Some(editor) => editor.view.doc,
                        None => {
                            return;
                        }
                    };
                    let path = doc
                        .get_untracked()
                        .content
                        .with_untracked(|content| content.path().cloned());
                    let path = match path {
                        Some(path) => path,
                        None => return,
                    };
                    self.common.internal_command.send(
                        InternalCommand::JumpToLocation {
                            location: EditorLocation {
                                path,
                                position: Some(EditorPosition::Position(
                                    range.start,
                                )),
                                scroll_offset: None,
                                ignore_unconfirmed: false,
                                same_editor_tab: false,
                            },
                        },
                    );
                }
                PaletteItemContent::WorkspaceSymbol { location, .. } => {
                    self.common.internal_command.send(
                        InternalCommand::JumpToLocation {
                            location: location.clone(),
                        },
                    );
                }
                PaletteItemContent::RunAndDebug { mode, config } => {
                    self.common.internal_command.send(
                        InternalCommand::RunAndDebug {
                            mode: *mode,
                            config: config.clone(),
                        },
                    );
                }
                PaletteItemContent::ColorTheme { name } => self
                    .common
                    .internal_command
                    .send(InternalCommand::SetColorTheme {
                        name: name.clone(),
                        save: true,
                    }),
                PaletteItemContent::IconTheme { name } => self
                    .common
                    .internal_command
                    .send(InternalCommand::SetIconTheme {
                        name: name.clone(),
                        save: true,
                    }),
                PaletteItemContent::Language { name } => {
                    let editor = self.main_split.active_editor.get_untracked();
                    let doc = match editor {
                        Some(editor) => editor.view.doc.get_untracked(),
                        None => {
                            return;
                        }
                    };
                    if name.is_empty() || name.to_lowercase().eq("plain text") {
                        doc.set_syntax(Syntax::plaintext())
                    } else {
                        let lang = match LapceLanguage::from_name(name) {
                            Some(v) => v,
                            None => return,
                        };
                        doc.set_language(lang);
                    }
                    doc.trigger_syntax_change(None);
                }
                PaletteItemContent::SCMReference { name } => {
                    self.common
                        .lapce_command
                        .send(crate::command::LapceCommand {
                        kind: CommandKind::Workbench(
                            crate::command::LapceWorkbenchCommand::CheckoutReference,
                        ),
                        data: Some(serde_json::json!(name.to_owned())),
                    });
                }
            }
        } else if self.kind.get_untracked() == PaletteKind::SshHost {
            let input = self.input.with_untracked(|input| input.input.clone());
            let ssh = SshHost::from_string(&input);
            self.common
                .window_command
                .send(WindowCommand::SetWorkspace {
                    workspace: LapceWorkspace {
                        kind: LapceWorkspaceType::RemoteSSH(ssh),
                        path: None,
                        last_open: 0,
                    },
                });
        }
    }

    /// Update the preview for the currently active palette item, if it has one.
    fn preview(&self, _cx: Scope) {
        if self.status.get_untracked() == PaletteStatus::Inactive {
            return;
        }

        let index = self.index.get_untracked();
        let items = self.filtered_items.get_untracked();
        if let Some(item) = items.get(index) {
            match &item.content {
                PaletteItemContent::PaletteHelp { .. } => {}
                PaletteItemContent::File { .. } => {}
                PaletteItemContent::Line { line, .. } => {
                    self.has_preview.set(true);
                    let editor = self.main_split.active_editor.get_untracked();
                    let doc = match editor {
                        Some(editor) => editor.view.doc.get_untracked(),
                        None => {
                            return;
                        }
                    };
                    let path = doc
                        .content
                        .with_untracked(|content| content.path().cloned());
                    let path = match path {
                        Some(path) => path,
                        None => return,
                    };
                    self.preview_editor.update_doc(doc);
                    self.preview_editor.go_to_location(
                        EditorLocation {
                            path,
                            position: Some(EditorPosition::Line(*line)),
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                        false,
                        None,
                    );
                }
                PaletteItemContent::Command { .. } => {}
                PaletteItemContent::Workspace { .. } => {}
                PaletteItemContent::RunAndDebug { .. } => {}
                PaletteItemContent::SshHost { .. } => {}
                PaletteItemContent::Language { .. } => {}
                PaletteItemContent::Reference { location, .. } => {
                    self.has_preview.set(true);
                    let (doc, new_doc) =
                        self.main_split.get_doc(location.path.clone());
                    self.preview_editor.update_doc(doc);
                    self.preview_editor.go_to_location(
                        location.clone(),
                        new_doc,
                        None,
                    );
                }
                PaletteItemContent::DocumentSymbol { range, .. } => {
                    self.has_preview.set(true);
                    let editor = self.main_split.active_editor.get_untracked();
                    let doc = match editor {
                        Some(editor) => editor.view.doc.get_untracked(),
                        None => {
                            return;
                        }
                    };
                    let path = doc
                        .content
                        .with_untracked(|content| content.path().cloned());
                    let path = match path {
                        Some(path) => path,
                        None => return,
                    };
                    self.preview_editor.update_doc(doc);
                    self.preview_editor.go_to_location(
                        EditorLocation {
                            path,
                            position: Some(EditorPosition::Position(range.start)),
                            scroll_offset: None,
                            ignore_unconfirmed: false,
                            same_editor_tab: false,
                        },
                        false,
                        None,
                    );
                }
                PaletteItemContent::WorkspaceSymbol { location, .. } => {
                    self.has_preview.set(true);
                    let (doc, new_doc) =
                        self.main_split.get_doc(location.path.clone());
                    self.preview_editor.update_doc(doc);
                    self.preview_editor.go_to_location(
                        location.clone(),
                        new_doc,
                        None,
                    );
                }
                PaletteItemContent::ColorTheme { name } => self
                    .common
                    .internal_command
                    .send(InternalCommand::SetColorTheme {
                        name: name.clone(),
                        save: false,
                    }),
                PaletteItemContent::IconTheme { name } => self
                    .common
                    .internal_command
                    .send(InternalCommand::SetIconTheme {
                        name: name.clone(),
                        save: false,
                    }),
                PaletteItemContent::SCMReference { .. } => {}
            }
        }
    }

    /// Cancel the palette, doing cleanup specific to the palette kind.
    fn cancel(&self) {
        if let PaletteKind::ColorTheme | PaletteKind::IconTheme =
            self.kind.get_untracked()
        {
            // TODO(minor): We don't really need to reload the *entire config* here!
            self.common
                .internal_command
                .send(InternalCommand::ReloadConfig);
        }

        self.close();
    }

    /// Close the palette, reverting focus back to the workbench.
    fn close(&self) {
        self.status.set(PaletteStatus::Inactive);
        if self.common.focus.get_untracked() == Focus::Palette {
            self.common.focus.set(Focus::Workbench);
        }
        self.has_preview.set(false);
        self.items.update(|items| items.clear());
        self.input_editor
            .view
            .doc
            .get_untracked()
            .reload(Rope::from(""), true);
        self.input_editor
            .cursor
            .update(|cursor| cursor.set_insert(Selection::caret(0)));
    }

    /// Move to the next entry in the palette list, wrapping around if needed.
    fn next(&self) {
        let index = self.index.get_untracked();
        let len = self.filtered_items.with_untracked(|i| i.len());
        let new_index = Movement::Down.update_index(index, len, 1, true);
        self.index.set(new_index);
    }

    /// Move to the previous entry in the palette list, wrapping around if needed.
    fn previous(&self) {
        let index = self.index.get_untracked();
        let len = self.filtered_items.with_untracked(|i| i.len());
        let new_index = Movement::Up.update_index(index, len, 1, true);
        self.index.set(new_index);
    }

    fn next_page(&self) {
        // TODO: implement
    }

    fn previous_page(&self) {
        // TODO: implement
    }

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            FocusCommand::ModalClose => {
                self.cancel();
            }
            FocusCommand::ListNext => {
                self.next();
            }
            FocusCommand::ListNextPage => {
                self.next_page();
            }
            FocusCommand::ListPrevious => {
                self.previous();
            }
            FocusCommand::ListPreviousPage => {
                self.previous_page();
            }
            FocusCommand::ListSelect => {
                self.select();
            }
            _ => return CommandExecuted::No,
        }
        CommandExecuted::Yes
    }

    fn filter_items(
        run_id: Arc<AtomicU64>,
        current_run_id: u64,
        input: &str,
        items: im::Vector<PaletteItem>,
        matcher: &mut nucleo::Matcher,
    ) -> Option<im::Vector<PaletteItem>> {
        if input.is_empty() {
            return Some(items);
        }

        let pattern = nucleo::pattern::Pattern::parse(
            input,
            nucleo::pattern::CaseMatching::Ignore,
        );

        // NOTE: We collect into a Vec to sort as we are hitting a worst-case behavior in
        // `im::Vector` that can lead to a stack overflow!
        let mut filtered_items = Vec::new();
        for i in &items {
            // If the run id has ever changed, then we'll just bail out of this filtering to avoid
            // wasting effort. This would happen, for example, on the user continuing to type.
            if run_id.load(std::sync::atomic::Ordering::Acquire) != current_run_id {
                return None;
            }

            let mut indices = Vec::new();
            let mut filter_text_buf = Vec::new();
            let filter_text = Utf32Str::new(&i.filter_text, &mut filter_text_buf);
            if let Some(score) = pattern.indices(filter_text, matcher, &mut indices)
            {
                let mut item = i.clone();
                item.score = score;
                item.indices = indices.into_iter().map(|i| i as usize).collect();
                filtered_items.push(item);
            }
        }

        filtered_items.sort_by(|a, b| {
            let order = b.score.cmp(&a.score);
            match order {
                std::cmp::Ordering::Equal => a.filter_text.cmp(&b.filter_text),
                _ => order,
            }
        });

        if run_id.load(std::sync::atomic::Ordering::Acquire) != current_run_id {
            return None;
        }
        Some(filtered_items.into())
    }

    fn update_process(
        run_id: Arc<AtomicU64>,
        receiver: Receiver<(u64, String, im::Vector<PaletteItem>)>,
        resp_tx: Sender<(u64, String, im::Vector<PaletteItem>)>,
    ) {
        fn receive_batch(
            receiver: &Receiver<(u64, String, im::Vector<PaletteItem>)>,
        ) -> Result<(u64, String, im::Vector<PaletteItem>)> {
            let (mut run_id, mut input, mut items) = receiver.recv()?;
            loop {
                match receiver.try_recv() {
                    Ok(update) => {
                        run_id = update.0;
                        input = update.1;
                        items = update.2;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }
            Ok((run_id, input, items))
        }

        let mut matcher =
            nucleo::Matcher::new(nucleo::Config::DEFAULT.match_paths());
        loop {
            if let Ok((current_run_id, input, items)) = receive_batch(&receiver) {
                if let Some(filtered_items) = Self::filter_items(
                    run_id.clone(),
                    current_run_id,
                    &input,
                    items,
                    &mut matcher,
                ) {
                    let _ = resp_tx.send((current_run_id, input, filtered_items));
                }
            } else {
                return;
            }
        }
    }
}

impl KeyPressFocus for PaletteData {
    fn get_mode(&self) -> lapce_core::mode::Mode {
        Mode::Insert
    }

    fn check_condition(
        &self,
        condition: crate::keypress::condition::Condition,
    ) -> bool {
        matches!(
            condition,
            Condition::ListFocus | Condition::PaletteFocus | Condition::ModalFocus
        )
    }

    fn run_command(
        &self,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: ModifiersState,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Focus(cmd) => {
                self.run_focus_command(cmd);
            }
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                self.input_editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::Yes
    }

    fn receive_char(&self, c: &str) {
        self.input_editor.receive_char(c);
    }
}
