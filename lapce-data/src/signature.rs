use std::{path::PathBuf, sync::Arc};

use druid::{Size, WidgetId};
use lapce_rpc::{buffer::BufferId, plugin::PluginId};
use lsp_types::{Position, SignatureHelp, SignatureInformation};

use crate::proxy::LapceProxy;

#[derive(Clone, PartialEq, Eq)]
pub enum SignatureStatus {
    Inactive,
    Started,
}

/// Data for the LSP Signature Help command, which displays information about the signature
/// of the function that that the user is calling.
#[derive(Clone)]
pub struct SignatureData {
    /// Id of the signature view widget
    pub id: WidgetId,
    /// Id of the scroll widget within the signature view widget
    pub scroll_id: WidgetId,
    pub request_id: usize,
    pub status: SignatureStatus,
    pub buffer_id: BufferId,
    /// The offset into the current buffer that the signature was being used at
    /// This is for positionng the signature information
    pub offset: usize,
    /// Size of the signature view
    pub size: Size,

    // TODO: Allow switching between the signatures.
    // (Since we request an update for the signature often, the LSP should swap
    // the active parameter if it gets a better idea of the type, which will cover most cases)
    pub signatures: Vec<SignatureInformation>,
    pub current_signature: usize,
    /// The currently parameter the user is editing
    pub active_parameter: Option<usize>,
}
impl SignatureData {
    pub fn new() -> Self {
        Self {
            id: WidgetId::next(),
            scroll_id: WidgetId::next(),
            request_id: 0,
            status: SignatureStatus::Inactive,
            buffer_id: BufferId(0),
            offset: 0,
            // TODO: Let the user customize this
            size: Size::new(400.0, 100.0),

            signatures: Vec::new(),
            current_signature: 0,
            active_parameter: None,
        }
    }

    /// Check if there are any signatures available
    pub fn is_empty(&self) -> bool {
        self.signatures.is_empty()
    }

    /// Get the currently active signature entry, if one exists
    pub fn current(&self) -> Option<&SignatureInformation> {
        self.signatures.get(self.current_signature)
    }

    pub fn request(
        &self,
        proxy: Arc<LapceProxy>,
        request_id: usize,
        path: PathBuf,
        position: Position,
    ) {
        proxy.proxy_rpc.signature_help(request_id, path, position);
    }

    pub fn cancel(&mut self) {
        if self.status == SignatureStatus::Inactive {
            return;
        }

        self.signatures.clear();
        self.current_signature = 0;
        self.active_parameter = None;
        self.status = SignatureStatus::Inactive;
    }

    pub fn receive(
        &mut self,
        request_id: usize,
        resp: SignatureHelp,
        _plugin_id: PluginId,
    ) {
        if self.status == SignatureStatus::Inactive || self.request_id != request_id
        {
            return;
        }

        let signatures = resp.signatures;
        let active_sig_idx = resp.active_signature.unwrap_or(0) as usize;
        let active_sig_idx = signatures
            .get(active_sig_idx)
            .map(|_| active_sig_idx)
            .unwrap_or(0);
        // TODO: If the active sig idx isn't defined then we can make a somewhat better than 0
        // guess by using the current active parameter to trim out any signatures with too
        // few parameters
        let active_parameter = resp.active_parameter.map(|idx| idx as usize);

        self.signatures = signatures;
        self.current_signature = active_sig_idx;
        self.active_parameter = active_parameter;

        // Updating of the text layouts for the UI is done in `SignatureContainer::update_signature`
    }
}

impl Default for SignatureData {
    fn default() -> Self {
        Self::new()
    }
}
