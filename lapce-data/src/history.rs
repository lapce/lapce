use std::{
    cell::RefCell,
    rc::Rc,
    sync::{atomic, Arc},
};

use druid::{
    piet::{PietText, PietTextLayout, Text, TextAttribute, TextLayoutBuilder},
    Target,
};
use lapce_core::{
    buffer::{rope_diff, Buffer, DiffLines},
    style::line_styles,
    syntax::Syntax,
};
use lapce_rpc::{
    buffer::BufferHeadResponse,
    style::{LineStyle, LineStyles, Style},
};
use xi_rope::{spans::Spans, Rope};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{Config, LapceTheme},
    document::{BufferContent, Document, TextLayoutCache},
};

#[derive(Clone)]
pub struct DocumentHistory {
    version: String,
    buffer: Option<Buffer>,
    styles: Arc<Spans<Style>>,
    line_styles: Rc<RefCell<LineStyles>>,
    changes: Arc<Vec<DiffLines>>,
    text_layouts: Rc<RefCell<TextLayoutCache>>,
}

impl druid::Data for DocumentHistory {
    fn same(&self, other: &Self) -> bool {
        if !self.changes.same(&other.changes) {
            return false;
        }

        if !self.styles.same(&other.styles) {
            return false;
        }

        match (self.buffer.as_ref(), other.buffer.as_ref()) {
            (None, None) => true,
            (None, Some(_)) | (Some(_), None) => false,
            (Some(buffer), Some(other_buffer)) => {
                buffer.text().ptr_eq(other_buffer.text())
            }
        }
    }
}

impl DocumentHistory {
    pub fn new(version: String) -> Self {
        Self {
            version,
            buffer: None,
            styles: Arc::new(Spans::default()),
            line_styles: Rc::new(RefCell::new(LineStyles::new())),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            changes: Arc::new(Vec::new()),
        }
    }

    pub fn load_content(&mut self, content: Rope, doc: &Document) {
        let mut buffer = Buffer::new("");
        buffer.init_content(content);
        self.buffer = Some(buffer);
        self.trigger_update_change(doc);
        self.retrieve_history_styles(doc);
    }

    pub fn get_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        config: &Config,
    ) -> Arc<PietTextLayout> {
        self.text_layouts.borrow_mut().check_attributes(config.id);
        if self.text_layouts.borrow().layouts.get(&line).is_none() {
            self.text_layouts
                .borrow_mut()
                .layouts
                .insert(line, Arc::new(self.new_text_layout(text, line, config)));
        }
        self.text_layouts
            .borrow()
            .layouts
            .get(&line)
            .cloned()
            .unwrap()
    }

    fn new_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        config: &Config,
    ) -> PietTextLayout {
        let line_content = self.buffer.as_ref().unwrap().line_content(line);
        let font_family = config.editor.font_family();
        let font_size = config.editor.font_size;
        let tab_width =
            config.tab_width(text, config.editor.font_family(), font_size);
        let mut layout_builder = text
            .new_text_layout(line_content.to_string())
            .font(font_family, font_size as f64)
            .text_color(
                config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .set_tab_width(tab_width);

        let styles = self.line_style(line);
        for line_style in styles.iter() {
            if let Some(fg_color) = line_style.style.fg_color.as_ref() {
                if let Some(fg_color) = config.get_style_color(fg_color) {
                    layout_builder = layout_builder.range_attribute(
                        line_style.start..line_style.end,
                        TextAttribute::TextColor(fg_color.clone()),
                    );
                }
            }
        }

        layout_builder.build().unwrap()
    }

    fn line_style(&self, line: usize) -> Arc<Vec<LineStyle>> {
        if self.line_styles.borrow().get(&line).is_none() {
            let line_styles = line_styles(
                self.buffer.as_ref().unwrap().text(),
                line,
                &self.styles,
            );
            self.line_styles
                .borrow_mut()
                .insert(line, Arc::new(line_styles));
        }
        self.line_styles.borrow().get(&line).cloned().unwrap()
    }

    pub fn retrieve(&self, doc: &Document) {
        if let BufferContent::File(path) = &doc.content() {
            let id = doc.id();
            let tab_id = doc.tab_id;
            let path = path.clone();
            let proxy = doc.proxy.clone();
            let event_sink = doc.event_sink.clone();
            std::thread::spawn(move || {
                proxy.get_buffer_head(
                    id,
                    path.clone(),
                    Box::new(move |result| {
                        if let Ok(res) = result {
                            if let Ok(resp) =
                                serde_json::from_value::<BufferHeadResponse>(res)
                            {
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::LoadBufferHead {
                                        path,
                                        content: Rope::from(resp.content),
                                        version: resp.version,
                                    },
                                    Target::Widget(tab_id),
                                );
                            }
                        }
                    }),
                )
            });
        }
    }

    pub fn trigger_update_change(&self, doc: &Document) {
        if self.buffer.is_none() {
            return;
        }
        if let BufferContent::File(path) = &doc.content() {
            let id = doc.id();
            let rev = doc.rev();
            let atomic_rev = doc.buffer().atomic_rev();
            let path = path.clone();
            let left_rope = self.buffer.as_ref().unwrap().text().clone();
            let right_rope = doc.buffer().text().clone();
            let event_sink = doc.event_sink.clone();
            let tab_id = doc.tab_id;
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

    pub fn changes(&self) -> &[DiffLines] {
        &self.changes
    }

    pub fn update_changes(&mut self, changes: Arc<Vec<DiffLines>>) {
        self.changes = changes;
    }

    pub fn update_styles(&mut self, styles: Arc<Spans<Style>>) {
        self.styles = styles;
        self.line_styles.borrow_mut().clear();
    }

    fn retrieve_history_styles(&self, doc: &Document) {
        if self.buffer.is_none() {
            return;
        }
        if let BufferContent::File(path) = &doc.content() {
            let id = doc.id();
            let path = path.clone();
            let tab_id = doc.tab_id;
            let version = self.version.to_string();
            let event_sink = doc.event_sink.clone();

            let content = self.buffer.as_ref().unwrap().text().clone();
            rayon::spawn(move || {
                if let Some(syntax) =
                    Syntax::init(&path).map(|s| s.parse(0, content, None))
                {
                    if let Some(styles) = syntax.styles {
                        let _ = event_sink.submit_command(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::UpdateHistoryStyle {
                                id,
                                path,
                                history: version,
                                highlights: styles,
                            },
                            Target::Widget(tab_id),
                        );
                    }
                }
            });
        }
    }
}
