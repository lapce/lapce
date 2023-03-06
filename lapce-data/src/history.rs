use std::{
    cell::RefCell,
    collections::HashMap,
    ops::Range,
    rc::Rc,
    sync::{atomic, Arc},
};

use druid::{
    piet::{PietText, Text, TextAttribute, TextLayoutBuilder},
    Target,
};
use itertools::Itertools;
use lapce_core::{
    buffer::{rope_diff, Buffer, DiffLines},
    style::line_styles,
    syntax::Syntax,
};
use lapce_rpc::{
    proxy::ProxyResponse,
    style::{LineStyle, LineStyles, Style},
};
use lapce_xi_rope::{spans::Spans, Rope};

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::{LapceConfig, LapceTheme},
    document::{BufferContent, Document, TextLayoutCache, TextLayoutLine},
};

#[derive(Clone)]
pub struct DocumentHistory {
    version: String,
    buffer: Option<Buffer>,
    styles: Arc<Spans<Style>>,
    line_styles: Rc<RefCell<LineStyles>>,
    changes: Arc<Vec<DiffLines>>,
    text_layouts: Rc<RefCell<TextLayoutCache>>,
    diff_context_lines: i32,
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
pub const DEFAULT_DIFF_CONTEXT_LINES: usize = 3;
impl DocumentHistory {
    pub fn new(version: String, diff_context_lines: i32) -> Self {
        Self {
            version,
            buffer: None,
            styles: Arc::new(Spans::default()),
            line_styles: Rc::new(RefCell::new(LineStyles::new())),
            text_layouts: Rc::new(RefCell::new(TextLayoutCache::new())),
            changes: Arc::new(Vec::new()),
            diff_context_lines,
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
        config: &LapceConfig,
    ) -> Arc<TextLayoutLine> {
        let font_size = 0;
        self.text_layouts.borrow_mut().check_attributes(config.id);
        if self.text_layouts.borrow().layouts.get(&font_size).is_none() {
            let mut cache = self.text_layouts.borrow_mut();
            cache.layouts.insert(font_size, HashMap::new());
        }
        if self
            .text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .unwrap()
            .get(&line)
            .is_none()
        {
            self.text_layouts
                .borrow_mut()
                .layouts
                .get_mut(&font_size)
                .unwrap()
                .insert(line, Arc::new(self.new_text_layout(text, line, config)));
        }
        self.text_layouts
            .borrow()
            .layouts
            .get(&font_size)
            .unwrap()
            .get(&line)
            .cloned()
            .unwrap()
    }

    fn new_text_layout(
        &self,
        text: &mut PietText,
        line: usize,
        config: &LapceConfig,
    ) -> TextLayoutLine {
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

        TextLayoutLine {
            text: layout_builder.build().unwrap(),
            extra_style: Vec::new(),
            whitespaces: None,
            indent: 0.0,
        }
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

    /// Retrieve the `head` version of the buffer
    pub fn retrieve(&self, doc: &Document) {
        if let BufferContent::File(path) = &doc.content() {
            let id = doc.id();
            let tab_id = doc.tab_id;
            let path = path.clone();
            let proxy = doc.proxy.clone();
            let event_sink = doc.event_sink.clone();
            std::thread::spawn(move || {
                proxy
                    .proxy_rpc
                    .get_buffer_head(id, path.clone(), move |result| {
                        if let Ok(ProxyResponse::BufferHeadResponse {
                            version,
                            content,
                        }) = result
                        {
                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::LoadBufferHead {
                                    path,
                                    content: Rope::from(content),
                                    version,
                                },
                                Target::Widget(tab_id),
                            );
                        }
                    })
            });
        }
    }

    pub fn trigger_increase_diff_extend_lines(
        &self,
        doc: &Document,
        diff_skip: DiffLines,
    ) {
        let incr = 5_usize;
        if self.buffer.is_none() {
            return;
        }
        if let BufferContent::File(path) = &doc.content() {
            let id = doc.id();
            let rev = doc.rev();
            let path = path.clone();
            let event_sink = doc.event_sink.clone();
            let tab_id = doc.tab_id;

            let old_changes = self.changes.clone();
            let changes_len = old_changes.len();
            let mut changes = self.changes.clone().to_vec();
            for (i, change) in old_changes.iter().enumerate() {
                if let DiffLines::Skip(left, right) = change {
                    if *change == diff_skip {
                        if i == 0 {
                            let l_incr =
                                if left.len() < incr { left.len() } else { incr };
                            let r_incr =
                                if right.len() < 5 { right.len() } else { incr };
                            changes[i] = DiffLines::Skip(
                                Range {
                                    start: left.start,
                                    end: left.end - l_incr,
                                },
                                Range {
                                    start: right.start,
                                    end: right.end - r_incr,
                                },
                            );
                            if let DiffLines::Both(l, r) = &old_changes[i + 1] {
                                changes[i + 1] = DiffLines::Both(
                                    Range {
                                        start: l.start - l_incr,
                                        end: l.end,
                                    },
                                    Range {
                                        start: r.start - r_incr,
                                        end: r.end,
                                    },
                                );
                            }
                        } else if i == changes_len - 1 {
                            let l_incr =
                                if left.len() < incr { left.len() } else { incr };
                            let r_incr =
                                if right.len() < 5 { right.len() } else { incr };
                            if let DiffLines::Both(l, r) = &old_changes[i - 1] {
                                changes[i - 1] = DiffLines::Both(
                                    Range {
                                        start: l.start,
                                        end: l.end + l_incr,
                                    },
                                    Range {
                                        start: r.start,
                                        end: r.end + r_incr,
                                    },
                                );
                            }
                            changes[i] = DiffLines::Skip(
                                Range {
                                    start: left.start + l_incr,
                                    end: left.end,
                                },
                                Range {
                                    start: right.start + r_incr,
                                    end: right.end,
                                },
                            );
                        } else {
                            let mut l_s_incr = incr; // left start increasement
                            let mut l_e_incr = incr; // left end increasement
                            let mut r_s_incr = incr; // right start increasement
                            let mut r_e_incr = incr; // right end increasement
                            if left.len() < incr * 2 {
                                l_s_incr = left.len();
                                l_e_incr = 0;
                            }
                            if right.len() < incr * 2 {
                                r_s_incr = right.len();
                                r_e_incr = 0;
                            }

                            if let DiffLines::Both(l, r) = &old_changes[i - 1] {
                                changes[i - 1] = DiffLines::Both(
                                    Range {
                                        start: l.start,
                                        end: l.end + l_s_incr,
                                    },
                                    Range {
                                        start: r.start,
                                        end: r.end + r_s_incr,
                                    },
                                );
                            }
                            changes[i] = DiffLines::Skip(
                                Range {
                                    start: left.start + l_s_incr,
                                    end: left.end - l_e_incr,
                                },
                                Range {
                                    start: right.start + r_s_incr,
                                    end: right.end - r_e_incr,
                                },
                            );
                            if let DiffLines::Both(l, r) = &old_changes[i + 1] {
                                changes[i + 1] = DiffLines::Both(
                                    Range {
                                        start: l.start - l_e_incr,
                                        end: l.end,
                                    },
                                    Range {
                                        start: r.start - r_e_incr,
                                        end: r.end,
                                    },
                                );
                            }
                        }
                    }
                }
            }
            let changes = changes
                .iter()
                .filter(|change| {
                    if let DiffLines::Skip(left, right) = change {
                        if left.is_empty() && right.is_empty() {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect_vec();
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::UpdateHistoryChanges {
                    id,
                    path,
                    rev,
                    history: "head".to_string(),
                    changes: Arc::new(changes),
                    diff_context_lines: self.diff_context_lines,
                },
                Target::Widget(tab_id),
            );
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
            let diff_context_lines = self.diff_context_lines;
            rayon::spawn(move || {
                let context_lines = if diff_context_lines == -1 {
                    // infinite context lines
                    None
                } else if diff_context_lines < 0 {
                    // default context lines
                    Some(DEFAULT_DIFF_CONTEXT_LINES)
                } else {
                    Some(diff_context_lines as usize)
                };
                if atomic_rev.load(atomic::Ordering::Acquire) != rev {
                    return;
                }
                let changes = rope_diff(
                    left_rope,
                    right_rope,
                    rev,
                    atomic_rev.clone(),
                    context_lines,
                );
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
                        diff_context_lines,
                    },
                    Target::Widget(tab_id),
                );
            });
        }
    }

    pub fn changes(&self) -> &[DiffLines] {
        &self.changes
    }

    pub fn diff_context_lines(&self) -> i32 {
        self.diff_context_lines
    }

    pub fn update_changes(
        &mut self,
        changes: Arc<Vec<DiffLines>>,
        diff_context_lines: i32,
    ) {
        self.changes = changes;
        self.diff_context_lines = diff_context_lines;
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
                if let Ok(mut syntax) = Syntax::init(&path) {
                    syntax.parse(0, content, None);
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
