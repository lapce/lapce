use std::path::PathBuf;

use lapce_core::buffer::Buffer;

#[derive(Clone)]
pub struct DocumentHistory {
    pub buffer: Buffer,
    // path: PathBuf,
    // version: String,
    // styles: Arc<Spans<Style>>,
    // line_styles: Rc<RefCell<LineStyles>>,
    // text_layouts: Rc<RefCell<TextLayoutCache>>,
}

impl DocumentHistory {
    pub fn new(_path: PathBuf, _version: String, content: &str) -> Self {
        Self {
            buffer: Buffer::new(content),
            // path,
            // version,
            // styles: Arc::new(Spans::default()),
            // line_styles: Rc::new(RefCell::new(LineStyles::new())),
            // text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
        }
    }
}
