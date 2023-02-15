use std::path::PathBuf;

use floem::{
    app::AppContext,
    reactive::{create_rw_signal, RwSignal},
};
use lapce_core::buffer::Buffer;

pub enum DocContent {
    /// A file at some location. This can be a remote path.
    File(PathBuf),
    /// A local document, which doens't need to be sync to the disk.
    Local,
}

#[derive(Clone)]
pub struct EditorData {
    doc: RwSignal<Document>,
}

impl EditorData {
    pub fn new_local(cx: AppContext) -> Self {
        let doc = Document::new_local(cx);
        let doc = create_rw_signal(cx.scope, doc);
        Self { doc }
    }
}

pub struct Document {
    buffer: RwSignal<Buffer>,
    content: DocContent,
}

impl Document {
    pub fn new_local(cx: AppContext) -> Self {
        let buffer = create_rw_signal(cx.scope, Buffer::new(""));
        Self {
            buffer,
            content: DocContent::Local,
        }
    }
}
