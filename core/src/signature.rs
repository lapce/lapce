use lsp_types::SignatureHelp;
use tree_sitter::Node;

use crate::command::LapceUICommand;
use crate::state::{LapceTabState, LAPCE_APP_STATE};

#[derive(Clone)]
pub struct SignatureState {
    pub offset: usize,
    pub signature: Option<SignatureHelp>,
    pub active: (usize, usize),
}

impl SignatureState {
    pub fn new() -> Self {
        Self {
            offset: 0,
            signature: None,
            active: (0, 0),
        }
    }

    pub fn show(
        &mut self,
        signature_offset: usize,
        state: LapceTabState,
        signature: SignatureHelp,
    ) -> Option<()> {
        let mut editor_split = state.editor_split.lock();
        let (offset, commas) = editor_split.signature_offset()?;
        if signature_offset != offset {
            return None;
        }
        let label = signature.signatures[0].label.clone();
        self.signature = Some(signature);

        let editor = editor_split.editors.get(&editor_split.active)?;
        let buffer_id = editor.buffer_id.clone()?;
        let buffer = editor_split.buffers.get(&buffer_id)?;
        let offset = editor.selection.get_cursor_offset();
        let mut index = 0;
        for (i, c) in commas.iter().enumerate() {
            if offset <= *c {
                index = i;
                break;
            }
        }
        let label_commas: Vec<usize> =
            label.match_indices(',').map(|i| i.0).collect();
        if label_commas.len() == 0 {
            return None;
        }
        self.active = if index == 0 {
            (label.find("(").unwrap(), label_commas[0])
        } else if index >= label_commas.len() {
            (
                label_commas[label_commas.len() - 1],
                label.find(")").unwrap(),
            )
        } else {
            (label_commas[index - 1], label_commas[index])
        };
        None
    }

    pub fn update(&mut self, offset: usize, commas: Vec<usize>) -> Option<()> {
        let signature = self.signature.as_ref()?;
        let label = signature.signatures[0].label.clone();
        let mut index = 0;
        for (i, c) in commas.iter().enumerate() {
            if offset <= *c {
                index = i;
                break;
            }
        }
        let label_commas: Vec<usize> =
            label.match_indices(',').map(|i| i.0).collect();
        if label_commas.len() == 0 {
            return None;
        }
        self.active = if index == 0 {
            (label.find("(").unwrap(), label_commas[0])
        } else if index >= label_commas.len() {
            (
                label_commas[label_commas.len() - 1],
                label.find(")").unwrap(),
            )
        } else {
            (label_commas[index - 1], label_commas[index])
        };
        None
    }

    pub fn clear(&mut self) {
        self.offset = 0;
        self.signature = None;
        self.active = (0, 0);
    }
}
