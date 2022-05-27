use crate::mode::VisualMode;

pub trait Clipboard {
    fn get_string(&self) -> Option<String>;
    fn put_string(&mut self, s: impl AsRef<str>);
}

#[derive(Clone, Default)]
pub struct RegisterData {
    pub content: String,
    pub mode: VisualMode,
}

#[derive(Clone, Default)]
pub struct Register {
    pub unamed: RegisterData,
    last_yank: RegisterData,
}

pub enum RegisterKind {
    Delete,
    Yank,
}

impl Register {
    pub fn add(&mut self, kind: RegisterKind, data: RegisterData) {
        match kind {
            RegisterKind::Delete => self.add_delete(data),
            RegisterKind::Yank => self.add_yank(data),
        }
    }

    pub fn add_delete(&mut self, data: RegisterData) {
        self.unamed = data;
    }

    pub fn add_yank(&mut self, data: RegisterData) {
        self.unamed = data.clone();
        self.last_yank = data;
    }
}
