use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    ops::Range,
    sync::{Arc, Weak},
};

use druid::{
    piet::HitTestPoint,
    text::{EditableText, ImeHandlerRef, InputHandler, Selection},
    Point, Rect,
};

#[derive(Debug, Clone, Copy, PartialEq)]
enum ImeLock {
    None,
    ReadWrite,
    Read,
}

pub struct ImeComponent {
    ime_session: Arc<RefCell<ImeSession>>,
    lock: Arc<Cell<ImeLock>>,
}

impl Default for ImeComponent {
    fn default() -> Self {
        let session = ImeSession {
            is_active: false,
            composition_range: None,
            text: "".to_string(),
            input_text: None,
            orgin: Point::ZERO,
            shift: 0,
        };
        ImeComponent {
            ime_session: Arc::new(RefCell::new(session)),
            lock: Arc::new(Cell::new(ImeLock::None)),
        }
    }
}

impl ImeComponent {
    pub fn ime_handler(&self) -> impl ImeHandlerRef {
        ImeSessionRef {
            inner: Arc::downgrade(&self.ime_session),
            lock: self.lock.clone(),
        }
    }

    /// Returns `true` if the inner [`ImeSession`] can be read.
    pub fn can_read(&self) -> bool {
        self.lock.get() != ImeLock::ReadWrite
    }

    pub fn borrow(&self) -> Ref<'_, ImeSession> {
        self.ime_session.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, ImeSession> {
        self.ime_session.borrow_mut()
    }

    pub fn set_origin(&self, origin: Point) {
        self.ime_session.borrow_mut().orgin = origin;
    }

    pub fn set_active(&mut self, active: bool) {
        self.ime_session.borrow_mut().is_active = active;
    }

    pub fn clear_text(&self) {
        self.ime_session.borrow_mut().text.clear();
    }

    pub fn get_input_text(&self) -> Option<String> {
        self.ime_session.borrow_mut().input_text.take()
    }

    pub fn get_shift(&self) -> usize {
        self.ime_session.borrow().shift
    }

    /// Returns `true` if the IME is actively composing (or the text is locked.)
    pub fn is_composing(&self) -> bool {
        self.can_read() && self.borrow().composition_range.is_some()
    }
}

impl ImeHandlerRef for ImeSessionRef {
    fn is_alive(&self) -> bool {
        Weak::strong_count(&self.inner) > 0
    }

    fn acquire(
        &self,
        mutable: bool,
    ) -> Option<Box<dyn druid::text::InputHandler + 'static>> {
        let lock = if mutable {
            ImeLock::ReadWrite
        } else {
            ImeLock::Read
        };
        self.lock.replace(lock);
        Weak::upgrade(&self.inner)
            .map(ImeSessionHandle::new)
            .map(|doc| Box::new(doc) as Box<dyn InputHandler>)
    }

    fn release(&self) -> bool {
        self.lock.replace(ImeLock::None) == ImeLock::ReadWrite
    }
}

struct ImeSessionRef {
    inner: Weak<RefCell<ImeSession>>,
    lock: Arc<Cell<ImeLock>>,
}

pub struct ImeSession {
    is_active: bool,
    /// The portion of the text that is currently marked by the IME.
    composition_range: Option<Range<usize>>,
    text: String,
    input_text: Option<String>,
    shift: usize,
    orgin: Point,
}

impl ImeSession {
    pub fn text(&self) -> &str {
        &self.text
    }
}

struct ImeSessionHandle {
    inner: Arc<RefCell<ImeSession>>,
    selection: Selection,
    text: String,
}

impl ImeSessionHandle {
    fn new(inner: Arc<RefCell<ImeSession>>) -> Self {
        let text = inner.borrow().text.clone();
        ImeSessionHandle {
            inner,
            text,
            selection: Selection::default(),
        }
    }
}

impl InputHandler for ImeSessionHandle {
    fn selection(&self) -> Selection {
        self.selection
    }

    fn set_selection(&mut self, selection: Selection) {
        self.selection = selection;
        self.inner.borrow_mut().shift = selection.active;
    }

    fn composition_range(&self) -> Option<std::ops::Range<usize>> {
        self.inner.borrow().composition_range.clone()
    }

    fn set_composition_range(&mut self, range: Option<std::ops::Range<usize>>) {
        if range.is_none() {
            self.inner.borrow_mut().text.clear();
            self.text.clear();
        }
        self.inner.borrow_mut().composition_range = range;
    }

    fn is_char_boundary(&self, i: usize) -> bool {
        self.text.cursor(i).is_some()
    }

    fn len(&self) -> usize {
        self.text.len()
    }

    fn slice(&self, range: std::ops::Range<usize>) -> std::borrow::Cow<str> {
        self.text.slice(range).unwrap()
    }

    fn insert_text(&mut self, text: &str) {
        if self.composition_range().is_some() {
            self.inner.borrow_mut().input_text = Some(text.to_string());
        }
        self.replace_range(0..0, "");
    }

    fn is_active(&self) -> bool {
        self.inner.borrow().is_active
    }

    fn replace_range(&mut self, _range: Range<usize>, text: &str) {
        self.inner.borrow_mut().text = text.to_string();
        self.text = self.inner.borrow().text.clone();
    }

    fn hit_test_point(&self, _point: Point) -> HitTestPoint {
        HitTestPoint::default()
    }

    fn line_range(
        &self,
        _index: usize,
        _affinity: druid::text::Affinity,
    ) -> std::ops::Range<usize> {
        0..self.len()
    }

    fn bounding_box(&self) -> Option<Rect> {
        None
    }

    fn slice_bounding_box(&self, _range: std::ops::Range<usize>) -> Option<Rect> {
        Some(Rect::ZERO.with_origin(self.inner.borrow().orgin))
    }

    fn handle_action(&mut self, _action: druid::text::TextAction) {}
}
