use floem::{
    Renderer, View, ViewId,
    context::PaintCx,
    peniko::kurbo::{Point, Rect, Size},
    reactive::{Memo, SignalGet, SignalWith},
    text::{Attrs, AttrsList, FamilyOwned, TextLayout},
};
use im::HashMap;
use lapce_core::{buffer::rope_text::RopeText, mode::Mode};
use serde::{Deserialize, Serialize};

use super::{EditorData, view::changes_colors_screen};
use crate::config::{LapceConfig, color::LapceColor};

pub struct EditorGutterView {
    id: ViewId,
    editor: EditorData,
    width: f64,
    gutter_padding_right: Memo<f32>,
}

pub fn editor_gutter_view(
    editor: EditorData,
    gutter_padding_right: Memo<f32>,
) -> EditorGutterView {
    let id = ViewId::new();

    EditorGutterView {
        id,
        editor,
        width: 0.0,
        gutter_padding_right,
    }
}

impl EditorGutterView {
    fn paint_head_changes(
        &self,
        cx: &mut PaintCx,
        e_data: &EditorData,
        viewport: Rect,
        is_normal: bool,
        config: &LapceConfig,
    ) {
        if !is_normal {
            return;
        }

        let changes = e_data.doc().head_changes().get_untracked();
        let line_height = config.editor.line_height() as f64;
        let gutter_padding_right = self.gutter_padding_right.get_untracked() as f64;

        let changes = changes_colors_screen(config, &e_data.editor, changes);
        for (y, height, removed, color) in changes {
            let height = if removed {
                10.0
            } else {
                height as f64 * line_height
            };
            let mut y = y - viewport.y0;
            if removed {
                y -= 5.0;
            }
            cx.fill(
                &Size::new(3.0, height).to_rect().with_origin(Point::new(
                    self.width + 5.0 - gutter_padding_right,
                    y,
                )),
                color,
                0.0,
            )
        }
    }

    fn paint_sticky_headers(
        &self,
        cx: &mut PaintCx,
        is_normal: bool,
        config: &LapceConfig,
    ) {
        if !is_normal {
            return;
        }

        if !config.editor.sticky_header {
            return;
        }
        let sticky_header_height = self.editor.sticky_header_height.get_untracked();
        if sticky_header_height == 0.0 {
            return;
        }

        let sticky_area_rect =
            Size::new(self.width + 25.0 + 30.0, sticky_header_height)
                .to_rect()
                .with_origin(Point::new(-25.0, 0.0))
                .inflate(25.0, 0.0);
        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::LAPCE_DROPDOWN_SHADOW),
            3.0,
        );
        cx.fill(
            &sticky_area_rect,
            config.color(LapceColor::EDITOR_STICKY_HEADER_BACKGROUND),
            0.0,
        );
    }
}

impl View for EditorGutterView {
    fn id(&self) -> ViewId {
        self.id
    }

    fn compute_layout(
        &mut self,
        _cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<floem::peniko::kurbo::Rect> {
        if let Some(width) = self.id.get_layout().map(|l| l.size.width as f64) {
            self.width = width;
        }
        None
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        let viewport = self.editor.viewport().get_untracked();
        let cursor = self.editor.cursor();
        let screen_lines = self.editor.screen_lines();
        let config = self.editor.common.config;

        let kind_is_normal =
            self.editor.kind.with_untracked(|kind| kind.is_normal());
        let (offset, mode) = cursor.with_untracked(|c| (c.offset(), c.get_mode()));
        let config = config.get_untracked();
        let line_height = config.editor.line_height() as f64;
        let last_line = self.editor.editor.last_line();
        let current_line = self
            .editor
            .doc()
            .buffer
            .with_untracked(|buffer| buffer.line_of_offset(offset));

        let family: Vec<FamilyOwned> =
            FamilyOwned::parse_list(&config.editor.font_family).collect();
        let attrs = Attrs::new()
            .family(&family)
            .color(config.color(LapceColor::EDITOR_DIM))
            .font_size(config.editor.font_size() as f32);
        let attrs_list = AttrsList::new(attrs.clone());
        let current_line_attrs_list = AttrsList::new(
            attrs
                .clone()
                .color(config.color(LapceColor::EDITOR_FOREGROUND)),
        );
        let show_relative = config.core.modal
            && config.editor.modal_mode_relative_line_numbers
            && mode != Mode::Insert
            && kind_is_normal;

        screen_lines.with_untracked(|screen_lines| {
            for (line, y) in screen_lines.iter_lines_y() {
                // If it ends up outside the bounds of the file, stop trying to display line numbers
                if line > last_line {
                    break;
                }

                let text = if show_relative {
                    if line == current_line {
                        line + 1
                    } else {
                        line.abs_diff(current_line)
                    }
                } else {
                    line + 1
                }
                .to_string();

                let mut text_layout = TextLayout::new();
                if line == current_line {
                    text_layout.set_text(
                        &text,
                        current_line_attrs_list.clone(),
                        None,
                    );
                } else {
                    text_layout.set_text(&text, attrs_list.clone(), None);
                }
                let size = text_layout.size();
                let height = size.height;

                cx.draw_text(
                    &text_layout,
                    Point::new(
                        (self.width
                            - size.width
                            - self.gutter_padding_right.get_untracked() as f64)
                            .max(0.0),
                        y + (line_height - height) / 2.0 - viewport.y0,
                    ),
                );
            }
        });

        self.paint_head_changes(cx, &self.editor, viewport, kind_is_normal, &config);
        self.paint_sticky_headers(cx, kind_is_normal, &config);
    }

    fn debug_name(&self) -> std::borrow::Cow<'static, str> {
        "Editor Gutter".into()
    }
}

#[derive(Default, Clone)]
pub struct FoldingRanges(pub Vec<FoldingRange>);

#[derive(Default, Clone)]
pub struct FoldedRanges(pub Vec<FoldedRange>);

impl FoldingRanges {
    pub fn get_folded_range(&self) -> FoldedRanges {
        let mut range = Vec::new();
        let mut limit_line = 0;
        for item in &self.0 {
            if item.start.line < limit_line && limit_line > 0 {
                continue;
            }
            if item.status.is_folded() {
                range.push(crate::editor::gutter::FoldedRange {
                    start: item.start,
                    end: item.end,
                });
                limit_line = item.end.line;
            }
        }

        FoldedRanges(range)
    }
    pub fn to_display_items(&self) -> Vec<FoldingDisplayItem> {
        let mut folded = HashMap::new();
        let mut unfold_start: HashMap<u32, FoldingDisplayItem> = HashMap::new();
        let mut unfold_end = HashMap::new();
        let mut limit_line = 0;
        for item in &self.0 {
            if item.start.line < limit_line && limit_line > 0 {
                continue;
            }
            match item.status {
                FoldingRangeStatus::Fold => {
                    folded.insert(
                        item.start.line,
                        FoldingDisplayItem::Folded(item.start),
                    );
                    limit_line = item.end.line;
                }
                FoldingRangeStatus::Unfold => {
                    unfold_start.insert(
                        item.start.line,
                        FoldingDisplayItem::UnfoldStart(item.start),
                    );
                    unfold_end.insert(
                        item.end.line,
                        FoldingDisplayItem::UnfoldEnd(item.end),
                    );
                    limit_line = 0;
                }
            };
        }
        for (key, val) in unfold_end {
            unfold_start.insert(key, val);
        }
        for (key, val) in folded {
            unfold_start.insert(key, val);
        }

        unfold_start.into_iter().map(|x| x.1).collect()
    }
}

impl FoldedRanges {
    pub fn contain_line(&self, start_index: usize, line: u32) -> (bool, usize) {
        if start_index >= self.0.len() {
            return (false, start_index);
        }
        let mut last_index = start_index;
        for range in self.0[start_index..].iter() {
            if range.start.line >= line {
                return (false, last_index);
            } else if range.start.line < line && range.end.line > line {
                return (true, last_index);
            } else if range.end.line < line {
                last_index += 1;
            }
        }
        (false, last_index)
    }
}

#[derive(Debug, Clone)]
pub struct FoldedRange {
    pub start: FoldingPosition,
    pub end: FoldingPosition,
}

#[derive(Debug, Clone)]
pub struct FoldingRange {
    pub start: FoldingPosition,
    pub end: FoldingPosition,
    pub status: FoldingRangeStatus,
    pub collapsed_text: Option<String>,
}

impl FoldingRange {
    pub fn from_lsp(value: lsp_types::FoldingRange) -> Self {
        let lsp_types::FoldingRange {
            start_line,
            start_character,
            end_line,
            end_character,
            collapsed_text,
            ..
        } = value;
        let status = FoldingRangeStatus::Unfold;
        Self {
            start: FoldingPosition {
                line: start_line,
                character: start_character,
                // kind: kind.clone().map(|x| FoldingRangeKind::from(x)),
            },
            end: FoldingPosition {
                line: end_line,
                character: end_character,
                // kind: kind.map(|x| FoldingRangeKind::from(x)),
            },
            status,
            collapsed_text,
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Copy)]
pub struct FoldingPosition {
    pub line: u32,
    pub character: Option<u32>,
    // pub kind: Option<FoldingRangeKind>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq)]
pub enum FoldingRangeStatus {
    Fold,
    #[default]
    Unfold,
}

impl FoldingRangeStatus {
    pub fn click(&mut self) {
        // match self {
        //     FoldingRangeStatus::Fold => {
        //         *self = FoldingRangeStatus::Unfold;
        //     }
        //     FoldingRangeStatus::Unfold => {
        //         *self = FoldingRangeStatus::Fold;
        //     }
        // }
    }
    pub fn is_folded(&self) -> bool {
        *self == Self::Fold
    }
}
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum FoldingDisplayItem {
    UnfoldStart(FoldingPosition),
    Folded(FoldingPosition),
    UnfoldEnd(FoldingPosition),
}

impl FoldingDisplayItem {
    pub fn position(&self) -> FoldingPosition {
        match self {
            FoldingDisplayItem::UnfoldStart(x) => *x,
            FoldingDisplayItem::Folded(x) => *x,
            FoldingDisplayItem::UnfoldEnd(x) => *x,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Deserialize, Serialize, Clone, Hash, Copy)]
pub enum FoldingRangeKind {
    Comment,
    Imports,
    Region,
}

impl From<lsp_types::FoldingRangeKind> for FoldingRangeKind {
    fn from(value: lsp_types::FoldingRangeKind) -> Self {
        match value {
            lsp_types::FoldingRangeKind::Comment => FoldingRangeKind::Comment,
            lsp_types::FoldingRangeKind::Imports => FoldingRangeKind::Imports,
            lsp_types::FoldingRangeKind::Region => FoldingRangeKind::Region,
        }
    }
}
