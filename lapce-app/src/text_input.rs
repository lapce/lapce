use std::sync::Arc;

use floem::{
    context::EventCx,
    cosmic_text::{
        Attrs, AttrsList, FamilyOwned, LineHeightValue, Style as FontStyle,
        TextLayout, Weight,
    },
    event::Event,
    id::Id,
    peniko::{
        kurbo::{Line, Point, Rect, Size},
        Color,
    },
    reactive::{
        create_effect, ReadSignal, RwSignal, SignalGet, SignalGetUntracked,
        SignalUpdate, SignalWith, SignalWithUntracked,
    },
    style::{ComputedStyle, Style},
    taffy::prelude::Node,
    view::{ChangeFlags, View},
    AppContext, Renderer,
};
use lapce_core::{
    cursor::{Cursor, CursorMode},
    selection::Selection,
};

use crate::{
    config::{color::LapceColor, LapceConfig},
    doc::Document,
};

pub fn text_input(
    doc: RwSignal<Document>,
    cursor: RwSignal<Cursor>,
    is_focused: impl Fn() -> bool + 'static,
    config: ReadSignal<Arc<LapceConfig>>,
) -> TextInput {
    let cx = AppContext::get_current();
    let id = cx.new_id();

    create_effect(cx.scope, move |_| {
        let content = doc.with(|doc| doc.buffer().to_string());
        id.update_state(TextInputState::Content(content), false);
    });

    create_effect(cx.scope, move |_| {
        cursor.with(|_| ());
        id.request_paint();
    });

    create_effect(cx.scope, move |_| {
        let focus = is_focused();
        id.update_state(TextInputState::Focus(focus), false);
    });

    TextInput {
        id,
        config,
        content: "".to_string(),
        focus: false,
        text_node: None,
        text_layout: None,
        cursor,
        doc,
        color: None,
        font_size: None,
        font_family: None,
        font_weight: None,
        font_style: None,
        cursor_pos: Point::ZERO,
        on_cursor_pos: None,
        line_height: None,
    }
}

enum TextInputState {
    Content(String),
    Focus(bool),
}

pub struct TextInput {
    id: Id,
    content: String,
    doc: RwSignal<Document>,
    cursor: RwSignal<Cursor>,
    focus: bool,
    text_node: Option<Node>,
    text_layout: Option<TextLayout>,
    color: Option<Color>,
    font_size: Option<f32>,
    font_family: Option<String>,
    font_weight: Option<Weight>,
    font_style: Option<FontStyle>,
    line_height: Option<LineHeightValue>,
    cursor_pos: Point,
    on_cursor_pos: Option<Box<dyn Fn(Point)>>,
    config: ReadSignal<Arc<LapceConfig>>,
}

impl TextInput {
    pub fn on_cursor_pos(mut self, cursor_pos: impl Fn(Point) + 'static) -> Self {
        self.on_cursor_pos = Some(Box::new(cursor_pos));
        self
    }

    fn set_text_layout(&mut self) {
        let mut text_layout = TextLayout::new();
        let mut attrs = Attrs::new().color(self.color.unwrap_or(Color::BLACK));
        if let Some(font_size) = self.font_size {
            attrs = attrs.font_size(font_size);
        }
        if let Some(font_style) = self.font_style {
            attrs = attrs.style(font_style);
        }
        let font_family = self.font_family.as_ref().map(|font_family| {
            let family: Vec<FamilyOwned> =
                FamilyOwned::parse_list(font_family).collect();
            family
        });
        if let Some(font_family) = font_family.as_ref() {
            attrs = attrs.family(font_family);
        }
        if let Some(font_weight) = self.font_weight {
            attrs = attrs.weight(font_weight);
        }
        if let Some(line_height) = self.line_height {
            attrs = attrs.line_height(line_height);
        }
        text_layout.set_text(
            if self.content.is_empty() {
                " "
            } else {
                self.content.as_str()
            },
            AttrsList::new(attrs),
        );
        self.text_layout = Some(text_layout);
    }

    fn hit_index(&self, cx: &mut EventCx, point: Point) -> usize {
        if let Some(text_layout) = self.text_layout.as_ref() {
            let padding_left = cx
                .get_computed_style(self.id)
                .map(|s| match s.padding_left {
                    floem::taffy::style::LengthPercentage::Points(v) => v,
                    floem::taffy::style::LengthPercentage::Percent(pct) => {
                        let layout = cx.get_layout(self.id()).unwrap();
                        pct * layout.size.width
                    }
                })
                .unwrap_or(0.0) as f64;
            let hit = text_layout.hit_point(Point::new(point.x - padding_left, 0.0));
            hit.index.min(self.content.len())
        } else {
            0
        }
    }
}

impl View for TextInput {
    fn id(&self) -> Id {
        self.id
    }

    fn child(&mut self, _id: Id) -> Option<&mut dyn View> {
        None
    }

    fn children(&mut self) -> Vec<&mut dyn View> {
        Vec::new()
    }

    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) -> ChangeFlags {
        if let Ok(state) = state.downcast() {
            match *state {
                TextInputState::Content(content) => {
                    self.content = content;
                    self.text_layout = None;
                }
                TextInputState::Focus(focus) => {
                    self.focus = focus;
                }
            }
            cx.request_layout(self.id);
            ChangeFlags::LAYOUT
        } else {
            ChangeFlags::empty()
        }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::Node {
        cx.layout_node(self.id, true, |cx| {
            if self.font_size != cx.current_font_size()
                || self.font_family.as_deref() != cx.current_font_family()
                || self.font_weight != cx.current_font_weight()
                || self.font_style != cx.current_font_style()
                || self.line_height != cx.current_line_height()
            {
                self.font_size = cx.current_font_size();
                self.font_family = cx.current_font_family().map(|s| s.to_string());
                self.font_weight = cx.current_font_weight();
                self.font_style = cx.current_font_style();
                self.line_height = cx.current_line_height();
                self.text_layout = None;
            }

            if self.text_layout.is_none() {
                self.set_text_layout();
            }
            let text_layout = self.text_layout.as_ref().unwrap();

            if let Some(on_cursor_pos) = self.on_cursor_pos.as_ref() {
                let offset = self.cursor.get_untracked().offset();
                let cursor_point = text_layout.hit_position(offset).point;
                if cursor_point != self.cursor_pos {
                    self.cursor_pos = cursor_point;
                    (*on_cursor_pos)(cursor_point);
                }
            }

            let size = text_layout.size();
            let width = size.width.ceil() as f32;
            let height = size.height as f32;

            if self.text_node.is_none() {
                self.text_node = Some(cx.new_node());
            }
            let text_node = self.text_node.unwrap();

            let style = Style::BASE
                .width_px(width)
                .height_px(height)
                .compute(&ComputedStyle::default())
                .to_taffy_style();
            cx.set_style(text_node, style);

            vec![text_node]
        })
    }

    fn compute_layout(&mut self, _cx: &mut floem::context::LayoutCx) {}

    fn event(
        &mut self,
        cx: &mut floem::context::EventCx,
        _id_path: Option<&[Id]>,
        event: floem::event::Event,
    ) -> bool {
        match event {
            Event::PointerDown(pointer) => {
                let offset = self.hit_index(cx, pointer.pos);
                self.cursor.update(|cursor| {
                    cursor.set_insert(Selection::caret(offset));
                });
                if pointer.button.is_left() && pointer.count == 2 {
                    let offset = self.hit_index(cx, pointer.pos);
                    let (start, end) = self
                        .doc
                        .with_untracked(|doc| doc.buffer().select_word(offset));
                    self.cursor.update(|cursor| {
                        cursor.set_insert(Selection::region(start, end));
                    });
                } else if pointer.button.is_left() && pointer.count == 3 {
                    self.cursor.update(|cursor| {
                        cursor.set_insert(Selection::region(0, self.content.len()));
                    });
                }
                cx.update_active(self.id);
            }
            Event::PointerMove(pointer) => {
                if cx.is_active(self.id) {
                    let offset = self.hit_index(cx, pointer.pos);
                    self.cursor.update(|cursor| {
                        cursor.set_offset(offset, true, false);
                    });
                }
            }
            _ => {}
        }
        false
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        if self.color != cx.current_color() {
            self.color = cx.current_color();
            self.set_text_layout();
        }

        let text_node = self.text_node.unwrap();
        let location = cx.layout(text_node).unwrap().location;
        let point = Point::new(location.x as f64, location.y as f64);
        let text_layout = self.text_layout.as_ref().unwrap();
        let height = text_layout.size().height;
        let config = self.config.get_untracked();

        let cursor = self.cursor.get_untracked();

        if let CursorMode::Insert(selection) = &cursor.mode {
            for region in selection.regions() {
                if !region.is_caret() {
                    let min = text_layout.hit_position(region.min()).point.x;
                    let max = text_layout.hit_position(region.max()).point.x;
                    cx.fill(
                        &Rect::ZERO
                            .with_size(Size::new(max - min, height))
                            .with_origin(Point::new(min + point.x, point.y)),
                        *config.get_color(LapceColor::EDITOR_SELECTION),
                    );
                }
            }
        }

        cx.draw_text(text_layout, point);

        if self.focus {
            let offset = cursor.offset();
            let hit_position = text_layout.hit_position(offset);
            let cursor_point = hit_position.point + point.to_vec2();
            cx.stroke(
                &Line::new(
                    Point::new(
                        cursor_point.x,
                        cursor_point.y - hit_position.glyph_ascent,
                    ),
                    Point::new(
                        cursor_point.x,
                        cursor_point.y + hit_position.glyph_descent,
                    ),
                ),
                *self
                    .config
                    .get_untracked()
                    .get_color(LapceColor::EDITOR_CARET),
                2.0,
            );
        }
    }
}
