use lsp_types::{ParameterLabel, SignatureHelp};

#[derive(Clone)]
pub struct SignatureState {
    pub offset: Option<usize>,
    pub signature: Option<SignatureHelp>,
    pub active: Option<(usize, usize)>,
}

impl SignatureState {
    pub fn new() -> Self {
        Self {
            offset: None,
            signature: None,
            active: None,
        }
    }

    pub fn update(&mut self, offset: usize, commas: Vec<usize>) -> Option<bool> {
        let signature = self.signature.as_ref()?;
        let params = signature.signatures[0].parameters.as_ref()?;
        if params.is_empty() {
            return None;
        }
        let mut index = commas.len();
        for (i, c) in commas.iter().enumerate() {
            if offset <= *c {
                index = i;
                break;
            }
        }

        let label = signature.signatures[0].label.clone();
        let active = if index >= params.len() {
            None
        } else {
            match &params[index].label {
                ParameterLabel::Simple(s) => {
                    let start = label.find(s)?;
                    Some((start, start + s.len()))
                }
                ParameterLabel::LabelOffsets(offsets) => {
                    Some((offsets[0] as usize, offsets[1] as usize))
                }
            }
        };

        let changed = self.active != active;
        self.active = active;
        Some(changed)
    }

    pub fn clear(&mut self) {
        self.offset = None;
        self.signature = None;
        self.active = None;
    }
}

impl Default for SignatureState {
    fn default() -> Self {
        Self::new()
    }
}
