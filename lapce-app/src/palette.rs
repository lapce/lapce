use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender, TryRecvError};
use floem::{
    app::AppContext,
    ext_event::{
        create_ext_action, create_signal_from_channel,
        create_signal_from_channel_oneshot,
    },
    reactive::{
        create_effect, create_memo, create_rw_signal, create_signal, ReadSignal,
        RwSignal, WriteSignal,
    },
};
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};
use lapce_core::{
    command::FocusCommand, mode::Mode, movement::Movement, register::Register,
};
use lapce_rpc::proxy::{ProxyResponse, ProxyRpcHandler};

use crate::{
    command::{CommandExecuted, CommandKind},
    config::LapceConfig,
    editor::EditorData,
    keypress::{condition::Condition, KeyPressFocus},
    workspace::LapceWorkspace,
};

use self::{
    item::{PaletteItem, PaletteItemContent},
    kind::PaletteKind,
};

pub mod item;
pub mod kind;

#[derive(Clone, PartialEq, Eq)]
pub enum PaletteStatus {
    Inactive,
    Started,
    Done,
}

#[derive(Clone)]
pub struct PaletteData {
    run_id: Arc<AtomicU64>,
    run_tx: Sender<(u64, String, im::Vector<PaletteItem>)>,
    pub workspace: Arc<LapceWorkspace>,
    pub status: RwSignal<PaletteStatus>,
    pub index: RwSignal<usize>,
    pub kind: RwSignal<PaletteKind>,
    pub items: RwSignal<im::Vector<PaletteItem>>,
    pub filtered_items: ReadSignal<im::Vector<PaletteItem>>,
    pub proxy_rpc: ProxyRpcHandler,
    pub editor: EditorData,
    pub config: ReadSignal<Arc<LapceConfig>>,
}

impl PaletteData {
    pub fn new(
        cx: AppContext,
        workspace: Arc<LapceWorkspace>,
        proxy_rpc: ProxyRpcHandler,
        register: RwSignal<Register>,
        config: ReadSignal<Arc<LapceConfig>>,
    ) -> Self {
        let status = create_rw_signal(cx.scope, PaletteStatus::Inactive);
        let kind = create_rw_signal(cx.scope, PaletteKind::File);
        let items = create_rw_signal(cx.scope, im::Vector::new());
        let index = create_rw_signal(cx.scope, 0);
        let editor = EditorData::new_local(cx, register, config);
        let run_id = Arc::new(AtomicU64::new(0));

        let (run_tx, run_rx) = crossbeam_channel::unbounded();
        {
            let run_id = run_id.clone();
            let doc = editor.doc.read_only();
            let items = items.read_only();
            let tx = run_tx.clone();
            create_effect(cx.scope, move |_| {
                let run_id = run_id.fetch_add(1, Ordering::Relaxed) + 1;
                let input = doc.with(|doc| doc.buffer().text().to_string());
                let items = items.get();
                let _ = tx.send((run_id, input, items));
            });
        }

        let (resp_tx, resp_rx) = crossbeam_channel::unbounded();
        {
            let run_id = run_id.clone();
            std::thread::spawn(move || {
                Self::update_process(run_id, run_rx, resp_tx);
            });
        }

        let (filtered_items, set_filtered_items) =
            create_signal(cx.scope, im::Vector::new());
        {
            let resp = create_signal_from_channel(cx, resp_rx);
            let run_id = run_id.clone();
            let index = index.write_only();
            create_effect(cx.scope, move |_| {
                if let Some((current_run_id, items)) = resp.get() {
                    if run_id.load(std::sync::atomic::Ordering::Acquire)
                        == current_run_id
                    {
                        set_filtered_items.set(items);
                        index.set(0);
                    }
                }
            });
        }

        Self {
            run_id,
            run_tx,
            workspace,
            status,
            kind,
            index,
            items,
            filtered_items,
            editor,
            proxy_rpc,
            config,
        }
    }

    pub fn run(&self, cx: AppContext, kind: PaletteKind) {
        match kind {
            PaletteKind::File => {
                self.get_files(cx);
            }
        }
        self.kind.set(kind);
    }

    fn get_files(&self, cx: AppContext) {
        let workspace = self.workspace.clone();
        let set_items = self.items.write_only();
        let send = create_ext_action(cx, move |items: Vec<PathBuf>| {
            let items = items
                .into_iter()
                .enumerate()
                .map(|(i, path)| {
                    let full_path = path.clone();
                    let mut path = path;
                    if let Some(workspace_path) = workspace.path.as_ref() {
                        path = path
                            .strip_prefix(workspace_path)
                            .unwrap_or(&full_path)
                            .to_path_buf();
                    }
                    let filter_text = path.to_str().unwrap_or("").to_string();
                    PaletteItem {
                        id: i,
                        content: PaletteItemContent::File { path, full_path },
                        filter_text,
                        score: 0,
                        indices: Vec::new(),
                    }
                })
                .collect::<im::Vector<_>>();
            set_items.set(items);
        });
        self.proxy_rpc.get_files(move |result| {
            if let Ok(ProxyResponse::GetFilesResponse { items }) = result {
                send(items);
            }
        });
    }

    fn next(&self) {
        let index = self.index.get();
        let len = self.filtered_items.with(|i| i.len());
        let new_index = Movement::Down.update_index(index, len, 1, true);
        self.index.set(new_index);
    }

    fn previous(&self) {
        let index = self.index.get();
        let len = self.filtered_items.with(|i| i.len());
        let new_index = Movement::Up.update_index(index, len, 1, true);
        self.index.set(new_index);
    }

    fn next_page(&self) {}

    fn previous_page(&self) {}

    fn run_focus_command(&self, cmd: &FocusCommand) -> CommandExecuted {
        match cmd {
            // ModalClose should be handled (if desired) by the containing widget
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
                // self.select(ctx);
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
        matcher: &SkimMatcherV2,
    ) -> Option<im::Vector<PaletteItem>> {
        if input.is_empty() {
            return Some(items);
        }

        // Collecting into a Vec to sort we as are hitting a worst case in
        // `im::Vector` that leads to a stack overflow
        let mut filtered_items = Vec::new();
        for i in &items {
            if run_id.load(std::sync::atomic::Ordering::Acquire) != current_run_id {
                return None;
            }
            if let Some((score, indices)) =
                matcher.fuzzy_indices(&i.filter_text, input)
            {
                let mut item = i.clone();
                item.score = score;
                item.indices = indices;
                filtered_items.push(item);
            }
        }

        filtered_items.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Less)
        });

        for (i, item) in filtered_items.iter_mut().enumerate() {
            item.id = i;
        }

        if run_id.load(std::sync::atomic::Ordering::Acquire) != current_run_id {
            return None;
        }
        Some(filtered_items.into())
    }

    fn update_process(
        run_id: Arc<AtomicU64>,
        receiver: Receiver<(u64, String, im::Vector<PaletteItem>)>,
        resp_tx: Sender<(u64, im::Vector<PaletteItem>)>,
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

        let matcher = SkimMatcherV2::default().ignore_case();
        loop {
            if let Ok((current_run_id, input, items)) = receive_batch(&receiver) {
                if let Some(filtered_items) = Self::filter_items(
                    run_id.clone(),
                    current_run_id,
                    &input,
                    items,
                    &matcher,
                ) {
                    let _ = resp_tx.send((current_run_id, filtered_items));
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
        matches!(condition, Condition::ListFocus | Condition::PaletteFocus)
    }

    fn run_command(
        &self,
        cx: AppContext,
        command: &crate::command::LapceCommand,
        count: Option<usize>,
        mods: floem::glazier::Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => todo!(),
            CommandKind::Edit(_) => {
                self.editor.run_command(cx, command, count, mods)
            }
            CommandKind::Move(_) => {
                self.editor.run_command(cx, command, count, mods)
            }
            CommandKind::Focus(cmd) => self.run_focus_command(cmd),
            CommandKind::MotionMode(_) => todo!(),
            CommandKind::MultiSelection(_) => todo!(),
        }
    }

    fn receive_char(&self, cx: AppContext, c: &str) {
        self.editor.receive_char(cx, c);
    }
}
