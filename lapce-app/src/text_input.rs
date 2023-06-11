use std::sync::Arc;

use floem::{
    context::EventCx,
    cosmic_text::{
        Attrs, AttrsList, FamilyOwned, LineHeightValue, Style as FontStyle,
        TextLayout, Weight,
    },
    event::{Event, EventListener},
    glazier::PointerType,
    id::Id,
    peniko::{
        kurbo::{Line, Point, Rect, Size, Vec2},
        Color,
    },
    reactive::{
        create_effect, ReadSignal, RwSignal, SignalGetUntracked, SignalSet,
        SignalUpdate, SignalWith, SignalWithUntracked,
    },
    style::{ComputedStyle, CursorStyle, Style},
    taffy::{self, prelude::Node},
    view::{ChangeFlags, View},
    views::Decorators,
    Renderer, ViewContext,
};
use lapce_core::{
    buffer::rope_text::RopeText,
    cursor::{Cursor, CursorMode},
    selection::Selection,
};

use crate::{
    config::{color::LapceColor, LapceConfig},
    doc::Document,
    editor::EditorData,
};

pub fn text_input(
    editor: EditorData,
    is_focused: impl Fn() -> bool + 'static,
) -> TextInput {
    let cx = ViewContext::get_current();
    let id = cx.new_id();

    let doc = editor.doc;
    let cursor = editor.cursor;
    let config = editor.common.config;
    let keypress = editor.common.keypress;

    create_effect(cx.scope, move |_| {
        let content = doc.with(|doc| doc.buffer().to_string());
        id.update_state(TextInputState::Content(content), false);
    });

    create_effect(cx.scope, move |_| {
        cursor.with(|_| ());
        id.request_layout();
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
        text_rect: Rect::ZERO,
        text_viewport: Rect::ZERO,
        placeholder: "".to_string(),
        placeholder_text_layout: None,
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
    .base_style(|| {
        Style::BASE
            .cursor(CursorStyle::Text)
            .padding_horiz_px(10.0)
            .padding_vert_px(6.0)
    })
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            let mut press = keypress.get_untracked();
            let executed = press.key_down(key_event, &editor);
            keypress.set(press);
            executed
        } else {
            false
        }
    })
}

enum TextInputState {
    Content(String),
    Focus(bool),
    Placeholder(String),
}

pub struct TextInput {
    id: Id,
    content: String,
    doc: RwSignal<Document>,
    cursor: RwSignal<Cursor>,
    focus: bool,
    text_node: Option<Node>,
    text_layout: Option<TextLayout>,
    text_rect: Rect,
    text_viewport: Rect,
    placeholder: String,
    placeholder_text_layout: Option<TextLayout>,
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
    pub fn placeholder(self, placeholder: impl Fn() -> String + 'static) -> Self {
        let cx = ViewContext::get_current();
        let id = self.id;
        create_effect(cx.scope, move |_| {
            let placeholder = placeholder();
            id.update_state(TextInputState::Placeholder(placeholder), false);
        });
        self
    }

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

        let mut placeholder_text_layout = TextLayout::new();
        attrs =
            attrs.color(self.color.unwrap_or(Color::BLACK).with_alpha_factor(0.5));
        placeholder_text_layout.set_text(&self.placeholder, AttrsList::new(attrs));
        self.placeholder_text_layout = Some(placeholder_text_layout);
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

    fn clamp_text_viewport(&mut self, text_viewport: Rect) {
        let text_rect = self.text_rect;
        let actual_size = text_rect.size();
        let width = text_rect.width();
        let height = text_rect.height();
        let child_size = self.text_layout.as_ref().unwrap().size();

        let mut text_viewport = text_viewport;
        if width >= child_size.width {
            text_viewport.x0 = 0.0;
        } else if text_viewport.x0 > child_size.width - width {
            text_viewport.x0 = child_size.width - width;
        } else if text_viewport.x0 < 0.0 {
            text_viewport.x0 = 0.0;
        }

        if height >= child_size.height {
            text_viewport.y0 = 0.0;
        } else if text_viewport.y0 > child_size.height - height {
            text_viewport.y0 = child_size.height - height;
        } else if text_viewport.y0 < 0.0 {
            text_viewport.y0 = 0.0;
        }

        let text_viewport = text_viewport.with_size(actual_size);
        if text_viewport != self.text_viewport {
            self.text_viewport = text_viewport;
            self.id.request_paint();
        }
    }

    fn ensure_cursor_visible(&mut self) {
        fn closest_on_axis(val: f64, min: f64, max: f64) -> f64 {
            assert!(min <= max);
            if val > min && val < max {
                0.0
            } else if val <= min {
                val - min
            } else {
                val - max
            }
        }

        let rect = Rect::ZERO.with_origin(self.cursor_pos).inflate(10.0, 0.0);
        // clamp the target region size to our own size.
        // this means we will show the portion of the target region that
        // includes the origin.
        let target_size = Size::new(
            rect.width().min(self.text_viewport.width()),
            rect.height().min(self.text_viewport.height()),
        );
        let rect = rect.with_size(target_size);

        let x0 = closest_on_axis(
            rect.min_x(),
            self.text_viewport.min_x(),
            self.text_viewport.max_x(),
        );
        let x1 = closest_on_axis(
            rect.max_x(),
            self.text_viewport.min_x(),
            self.text_viewport.max_x(),
        );
        let y0 = closest_on_axis(
            rect.min_y(),
            self.text_viewport.min_y(),
            self.text_viewport.max_y(),
        );
        let y1 = closest_on_axis(
            rect.max_y(),
            self.text_viewport.min_y(),
            self.text_viewport.max_y(),
        );

        let delta_x = if x0.abs() > x1.abs() { x0 } else { x1 };
        let delta_y = if y0.abs() > y1.abs() { y0 } else { y1 };
        let new_origin = self.text_viewport.origin() + Vec2::new(delta_x, delta_y);
        self.clamp_text_viewport(self.text_viewport.with_origin(new_origin));
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
                TextInputState::Placeholder(placeholder) => {
                    self.placeholder = placeholder;
                    self.placeholder_text_layout = None;
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
                self.placeholder_text_layout = None;
            }

            if self.text_layout.is_none() || self.placeholder_text_layout.is_none() {
                self.set_text_layout();
            }
            let text_layout = self.text_layout.as_ref().unwrap();

            let offset = self.cursor.get_untracked().offset();
            let cursor_point = text_layout.hit_position(offset).point;
            if cursor_point != self.cursor_pos {
                self.cursor_pos = cursor_point;
                self.ensure_cursor_visible();
            }

            let text_layout = self.text_layout.as_ref().unwrap();
            let size = text_layout.size();
            let height = size.height as f32;

            if self.text_node.is_none() {
                self.text_node = Some(cx.new_node());
            }
            let text_node = self.text_node.unwrap();

            let style = Style::BASE
                .height_px(height)
                .compute(&ComputedStyle::default())
                .to_taffy_style();
            cx.set_style(text_node, style);

            vec![text_node]
        })
    }

    fn compute_layout(&mut self, cx: &mut floem::context::LayoutCx) -> Option<Rect> {
        let layout = cx.get_layout(self.id).unwrap();

        let style = cx.get_computed_style(self.id);
        let padding_left = match style.padding_left {
            taffy::style::LengthPercentage::Points(padding) => padding,
            taffy::style::LengthPercentage::Percent(pct) => pct * layout.size.width,
        };
        let padding_right = match style.padding_right {
            taffy::style::LengthPercentage::Points(padding) => padding,
            taffy::style::LengthPercentage::Percent(pct) => pct * layout.size.width,
        };
        let padding_top = match style.padding_top {
            taffy::style::LengthPercentage::Points(padding) => padding,
            taffy::style::LengthPercentage::Percent(pct) => pct * layout.size.width,
        };
        let padding_bottom = match style.padding_bottom {
            taffy::style::LengthPercentage::Points(padding) => padding,
            taffy::style::LengthPercentage::Percent(pct) => pct * layout.size.width,
        };

        let size = Size::new(layout.size.width as f64, layout.size.height as f64);
        let mut text_rect = size.to_rect();
        text_rect.x0 += padding_left as f64;
        text_rect.x1 -= padding_right as f64;
        text_rect.y0 += padding_top as f64;
        text_rect.y1 -= padding_bottom as f64;
        self.text_rect = text_rect;

        self.clamp_text_viewport(self.text_viewport);

        None
    }

    fn event(
        &mut self,
        cx: &mut floem::context::EventCx,
        _id_path: Option<&[Id]>,
        event: floem::event::Event,
    ) -> bool {
        let text_offset = self.text_viewport.origin();
        let event = event.offset((-text_offset.x, -text_offset.y));
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
            Event::PointerWheel(pointer_event) => {
                let delta =
                    if let PointerType::Mouse(info) = pointer_event.pointer_type {
                        info.wheel_delta
                    } else {
                        Vec2::ZERO
                    };
                self.clamp_text_viewport(self.text_viewport + delta);
                return true;
            }
            _ => {}
        }
        false
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        cx.save();
        cx.clip(&self.text_rect.inflate(1.0, 2.0));
        if self.color != cx.current_color() {
            self.color = cx.current_color();
            self.set_text_layout();
        }

        let text_node = self.text_node.unwrap();
        let location = cx.layout(text_node).unwrap().location;
        let point = Point::new(location.x as f64, location.y as f64)
            - self.text_viewport.origin().to_vec2();
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

        if !self.content.is_empty() {
            cx.draw_text(text_layout, point);
        } else if !self.placeholder.is_empty() {
            cx.draw_text(self.placeholder_text_layout.as_ref().unwrap(), point);
        }

        if self.focus || cx.is_focused(self.id) {
            cx.clip(&self.text_rect.inflate(2.0, 2.0));
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
        cx.restore();
    }
}
