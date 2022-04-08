use druid::{ExtEventSink, Target, WidgetId};
use lapce_core::syntax::Syntax;
use lapce_rpc::style::{LineStyles, Style};
use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    sync::{
        atomic::{self},
        Arc,
    },
};
use xi_rope::{rope::Rope, spans::Spans, RopeDelta};

use crate::{
    buffer::{data::BufferData, rope_diff, BufferContent, LocalBufferKind},
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    find::{Find, FindProgress},
};

#[derive(Clone)]
pub struct BufferDecoration {
    pub(super) loaded: bool,
    pub(super) local: bool,

    pub(super) find: Rc<RefCell<Find>>,
    pub(super) find_progress: Rc<RefCell<FindProgress>>,

    pub(super) syntax: Option<Syntax>,
    pub(super) line_styles: Rc<RefCell<LineStyles>>,
    pub(super) semantic_styles: Option<Arc<Spans<Style>>>,

    pub(super) histories: im::HashMap<String, Rope>,

    pub(super) tab_id: WidgetId,
    pub(super) event_sink: ExtEventSink,
}

impl BufferDecoration {
    pub fn update_styles(&mut self, delta: &RopeDelta) {
        if let Some(styles) = self.semantic_styles.as_mut() {
            Arc::make_mut(styles).apply_shape(delta);
        } else if let Some(syntax) = self.syntax.as_mut() {
            if let Some(styles) = syntax.styles.as_mut() {
                Arc::make_mut(styles).apply_shape(delta);
            }
        }

        if let Some(syntax) = self.syntax.as_mut() {
            syntax.lens.apply_delta(delta);
        }

        self.line_styles.borrow_mut().clear();
    }

    pub fn notify_special(&self, buffer: &BufferData) {
        match &buffer.content {
            BufferContent::File(_) => {}
            BufferContent::Local(local) => {
                let s = buffer.rope.to_string();
                match local {
                    LocalBufferKind::Search => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateSearch(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::SourceControl => {}
                    LocalBufferKind::Empty => {}
                    LocalBufferKind::FilePicker => {
                        let pwd = PathBuf::from(s);
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdatePickerPwd(pwd),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::Keymap => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateKeymapsFilter(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                    LocalBufferKind::Settings => {
                        let _ = self.event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateSettingsFilter(s),
                            Target::Widget(self.tab_id),
                        );
                    }
                }
            }
            BufferContent::Value(_) => {}
        }
    }

    pub fn notify_update(&self, buffer: &BufferData, delta: Option<&RopeDelta>) {
        self.trigger_syntax_change(buffer, delta);
        self.trigger_history_change(buffer);
    }

    fn trigger_syntax_change(&self, buffer: &BufferData, delta: Option<&RopeDelta>) {
        if let BufferContent::File(path) = &buffer.content {
            if let Some(syntax) = self.syntax.clone() {
                let path = path.clone();
                let rev = buffer.rev;
                let text = buffer.rope.clone();
                let delta = delta.cloned();
                let atomic_rev = buffer.atomic_rev.clone();
                let event_sink = self.event_sink.clone();
                let tab_id = self.tab_id;
                rayon::spawn(move || {
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }
                    let new_syntax = syntax.parse(rev, text, delta);
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSyntax {
                            path,
                            rev,
                            syntax: new_syntax,
                        },
                        Target::Widget(tab_id),
                    );
                });
            }
        }
    }

    fn trigger_history_change(&self, buffer: &BufferData) {
        if let BufferContent::File(path) = &buffer.content {
            if let Some(head) = self.histories.get("head") {
                let id = buffer.id;
                let rev = buffer.rev;
                let atomic_rev = buffer.atomic_rev.clone();
                let path = path.clone();
                let left_rope = head.clone();
                let right_rope = buffer.rope.clone();
                let event_sink = self.event_sink.clone();
                let tab_id = self.tab_id;
                rayon::spawn(move || {
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }
                    let changes =
                        rope_diff(left_rope, right_rope, rev, atomic_rev.clone());
                    if changes.is_none() {
                        return;
                    }
                    let changes = changes.unwrap();
                    if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                        return;
                    }

                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateHistoryChanges {
                            id,
                            path,
                            rev,
                            history: "head".to_string(),
                            changes: Arc::new(changes),
                        },
                        Target::Widget(tab_id),
                    );
                });
            }
        }
    }
}
