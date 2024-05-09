use std::{rc::Rc, sync::Arc};

use floem::{
    action::{set_ime_allowed, set_ime_cursor_area},
    context::EventCx,
    cosmic_text::{Attrs, AttrsList, FamilyOwned, TextLayout},
    event::EventPropagation,
    event::{Event, EventListener},
    peniko::{
        kurbo::{Line, Point, Rect, Size, Vec2},
        Color,
    },
    prop_extractor,
    reactive::{
        create_effect, create_memo, create_rw_signal, Memo, ReadSignal, RwSignal,
        Scope,
    },
    style::{
        CursorStyle, FontFamily, FontSize, FontStyle, FontWeight, LineHeight,
        PaddingLeft, Style, TextColor,
    },
    taffy::prelude::NodeId,
    unit::PxPct,
    views::Decorators,
    Renderer, View, ViewId,
};
use lapce_core::{
    buffer::rope_text::RopeText,
    cursor::{Cursor, CursorMode},
    selection::Selection,
};
use lapce_xi_rope::Rope;

use crate::{
    config::{color::LapceColor, LapceConfig},
    doc::Doc,
    editor::{view::editor_style, DocSignal, EditorData},
    keypress::KeyPressFocus,
    main_split::Editors,
    window_tab::CommonData,
};

prop_extractor! {
    Extractor {
        color: TextColor,
        font_size: FontSize,
        font_family: FontFamily,
        font_weight: FontWeight,
        font_style: FontStyle,
        line_height: LineHeight,
    }
}

/// Builder for creating a [`TextInput`] easily.
pub struct TextInputBuilder {
    is_focused: Option<Memo<bool>>,
    // TODO: it'd be nice to not need to box this
    key_focus: Option<Box<dyn KeyPressFocus>>,
    value: Option<Rope>,
    keyboard_focus: RwSignal<bool>,
}

impl Default for TextInputBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TextInputBuilder {
    pub fn new() -> Self {
        Self {
            is_focused: None,
            key_focus: None,
            value: None,
            keyboard_focus: create_rw_signal(false),
        }
    }

    pub fn is_focused(mut self, is_focused: impl Fn() -> bool + 'static) -> Self {
        let keyboard_focus = self.keyboard_focus;
        self.is_focused =
            Some(create_memo(move |_| is_focused() || keyboard_focus.get()));
        self
    }

    /// Initialize with a specific value.  
    /// If this is set it will apply the value via reloading the editor's doc as pristine.
    pub fn value(mut self, value: impl Into<Rope>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn key_focus(mut self, key_focus: impl KeyPressFocus + 'static) -> Self {
        self.key_focus = Some(Box::new(key_focus));
        self
    }

    pub fn build(
        self,
        cx: Scope,
        editors: Editors,
        common: Rc<CommonData>,
    ) -> TextInput {
        let editor = editors.make_local(cx, common);
        let id = editor.id();

        self.build_editor(editor).on_cleanup(move || {
            editors.remove(id);
        })
    }

    /// Build the text input with a specific editor.  
    /// This function does *not* perform add/cleanup the editor to/from [`Editors`]
    pub fn build_editor(self, editor: EditorData) -> TextInput {
        let keyboard_focus = self.keyboard_focus;
        let is_focused = if let Some(is_focused) = self.is_focused {
            is_focused
        } else {
            create_memo(move |_| keyboard_focus.get())
        };

        if let Some(value) = self.value {
            editor.doc().reload(value, true);
        }

        text_input_full(editor, self.key_focus, is_focused, keyboard_focus)
    }
}

/// Create a basic single line text input  
/// `e_data` is the editor data that this input is associated with.  
/// `supplied_editor`
/// `key_focus` is what receives the keydown events, leave as `None` to default to editor.  
/// `is_focused` is a function that returns if the input is focused, used for certain events.
fn text_input_full<T: KeyPressFocus + 'static>(
    e_data: EditorData,
    key_focus: Option<T>,
    is_focused: Memo<bool>,
    keyboard_focus: RwSignal<bool>,
) -> TextInput {
    let id = ViewId::new();

    let doc = e_data.doc_signal();
    let cursor = e_data.cursor();
    let config = e_data.common.config;
    let keypress = e_data.common.keypress;
    let window_origin = create_rw_signal(Point::ZERO);
    let cursor_line = create_rw_signal(Line::new(Point::ZERO, Point::ZERO));
    let local_editor = e_data.clone();
    let editor = local_editor.editor.clone();

    {
        let doc = doc.get();
        create_effect(move |_| {
            let offset = cursor.with(|c| c.offset());
            let (content, offset, preedit_range) = {
                let content = doc.buffer.with(|b| b.to_string());
                if let Some(preedit) = doc.preedit.preedit.get().as_ref() {
                    let mut new_content = String::new();
                    new_content.push_str(&content[..offset]);
                    new_content.push_str(&preedit.text);
                    new_content.push_str(&content[offset..]);
                    let range = (offset, offset + preedit.text.len());
                    let offset = preedit
                        .cursor
                        .as_ref()
                        .map(|(_, end)| offset + *end)
                        .unwrap_or(offset);
                    (new_content, offset, Some(range))
                } else {
                    (content, offset, None)
                }
            };
            id.update_state(TextInputState::Content {
                text: content,
                offset,
                preedit_range,
            });
        });
    }

    {
        create_effect(move |_| {
            let focus = is_focused.get();
            id.update_state(TextInputState::Focus(focus));
        });

        let editor = editor.clone();
        let ime_allowed = editor.ime_allowed;
        create_effect(move |_| {
            let focus = is_focused.get();
            if focus {
                if !ime_allowed.get_untracked() {
                    ime_allowed.set(true);
                    set_ime_allowed(true);
                }
                let cursor_line = cursor_line.get();

                let window_origin = window_origin.get();
                let viewport = editor.viewport.get();
                let origin = window_origin
                    + Vec2::new(
                        cursor_line.p1.x - viewport.x0,
                        cursor_line.p1.y - viewport.y0,
                    );
                set_ime_cursor_area(origin, Size::new(800.0, 600.0));
            }
        });
    }

    let common_keyboard_focus = e_data.common.keyboard_focus;

    let ed1 = editor.clone();
    let ed2 = editor.clone();
    TextInput {
        id,
        config,
        offset: 0,
        preedit_range: None,
        layout_rect: Rect::ZERO,
        content: "".to_string(),
        focus: false,
        text_node: None,
        text_layout: create_rw_signal(None),
        text_rect: Rect::ZERO,
        text_viewport: Rect::ZERO,
        cursor_line,
        placeholder: "".to_string(),
        placeholder_text_layout: None,
        editor: e_data.clone(),
        cursor_pos: Point::ZERO,
        on_cursor_pos: None,
        hide_cursor: editor.cursor_info.hidden,
        style: Default::default(),
    }
    .style(move |s| {
        editor_style(config, doc, s)
            .cursor(CursorStyle::Text)
            .padding_horiz(10.0)
            .padding_vert(6.0)
    })
    .on_move(move |pos| {
        window_origin.set(pos);
    })
    .on_event_stop(EventListener::FocusGained, move |_| {
        keyboard_focus.set(true);
        common_keyboard_focus.set(Some(id));
    })
    .on_event_stop(EventListener::FocusLost, move |_| {
        keyboard_focus.set(false);
        if common_keyboard_focus.get_untracked() == Some(id) {
            common_keyboard_focus.set(None);
        }
    })
    .on_event(EventListener::KeyDown, move |event| {
        if let Event::KeyDown(key_event) = event {
            let keypress = keypress.get_untracked();
            let key_focus = key_focus
                .as_ref()
                .map(|k| k as &dyn KeyPressFocus)
                .unwrap_or(&e_data);
            if keypress.key_down(key_event, key_focus) {
                EventPropagation::Stop
            } else {
                EventPropagation::Continue
            }
        } else {
            EventPropagation::Continue
        }
    })
    .on_event(EventListener::ImePreedit, move |event| {
        if !is_focused.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImePreedit {
            text,
            cursor: ime_cursor,
        } = event
        {
            if text.is_empty() {
                ed1.clear_preedit();
            } else {
                let offset = cursor.with_untracked(|c| c.offset());
                ed1.set_preedit(text.clone(), ime_cursor.to_owned(), offset);
            }
        }
        EventPropagation::Stop
    })
    .on_event(EventListener::ImeCommit, move |event| {
        if !is_focused.get_untracked() {
            return EventPropagation::Continue;
        }

        if let Event::ImeCommit(text) = event {
            ed2.clear_preedit();
            ed2.receive_char(text.as_str());
        }
        EventPropagation::Stop
    })
}

enum TextInputState {
    Content {
        text: String,
        offset: usize,
        preedit_range: Option<(usize, usize)>,
    },
    Focus(bool),
    Placeholder(String),
}

pub struct TextInput {
    id: ViewId,
    content: String,
    offset: usize,
    preedit_range: Option<(usize, usize)>,
    editor: EditorData,
    focus: bool,
    text_node: Option<NodeId>,
    text_layout: RwSignal<Option<TextLayout>>,
    text_rect: Rect,
    text_viewport: Rect,
    layout_rect: Rect,
    cursor_line: RwSignal<Line>,
    placeholder: String,
    placeholder_text_layout: Option<TextLayout>,
    cursor_pos: Point,
    on_cursor_pos: Option<Box<dyn Fn(Point)>>,
    hide_cursor: RwSignal<bool>,
    config: ReadSignal<Arc<LapceConfig>>,
    style: Extractor,
}

impl TextInput {
    pub fn placeholder(self, placeholder: impl Fn() -> String + 'static) -> Self {
        let id = self.id;
        create_effect(move |_| {
            let placeholder = placeholder();
            id.update_state(TextInputState::Placeholder(placeholder));
        });
        self
    }

    pub fn on_cursor_pos(mut self, cursor_pos: impl Fn(Point) + 'static) -> Self {
        self.on_cursor_pos = Some(Box::new(cursor_pos));
        self
    }

    pub fn editor(&self) -> EditorData {
        self.editor.clone()
    }

    pub fn doc_signal(&self) -> DocSignal {
        self.editor.doc_signal()
    }

    pub fn doc(&self) -> Rc<Doc> {
        self.editor.doc()
    }

    pub fn cursor(&self) -> RwSignal<Cursor> {
        self.editor.cursor()
    }

    fn set_text_layout(&mut self) {
        let mut text_layout = TextLayout::new();
        let mut attrs =
            Attrs::new().color(self.style.color().unwrap_or(Color::BLACK));
        if let Some(font_size) = self.style.font_size() {
            attrs = attrs.font_size(font_size);
        }
        if let Some(font_style) = self.style.font_style() {
            attrs = attrs.style(font_style);
        }
        let font_family = self.style.font_family().as_ref().map(|font_family| {
            let family: Vec<FamilyOwned> =
                FamilyOwned::parse_list(font_family).collect();
            family
        });
        if let Some(font_family) = font_family.as_ref() {
            attrs = attrs.family(font_family);
        }
        if let Some(font_weight) = self.style.font_weight() {
            attrs = attrs.weight(font_weight);
        }
        if let Some(line_height) = self.style.line_height() {
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
        self.text_layout.set(Some(text_layout));

        let mut placeholder_text_layout = TextLayout::new();
        attrs = attrs.color(
            self.style
                .color()
                .unwrap_or(Color::BLACK)
                .with_alpha_factor(0.5),
        );
        placeholder_text_layout.set_text(&self.placeholder, AttrsList::new(attrs));
        self.placeholder_text_layout = Some(placeholder_text_layout);
    }

    fn hit_index(&self, _cx: &mut EventCx, point: Point) -> usize {
        self.text_layout.with_untracked(|text_layout| {
            if let Some(text_layout) = text_layout.as_ref() {
                let padding_left =
                    match self.id.get_combined_style().get(PaddingLeft) {
                        PxPct::Px(v) => v,
                        PxPct::Pct(pct) => {
                            let layout = self.id.get_layout().unwrap_or_default();
                            pct * layout.size.width as f64
                        }
                    };
                let hit =
                    text_layout.hit_point(Point::new(point.x - padding_left, 0.0));
                hit.index.min(self.content.len())
            } else {
                0
            }
        })
    }

    fn clamp_text_viewport(&mut self, text_viewport: Rect) {
        let text_rect = self.text_rect;
        let actual_size = text_rect.size();
        let width = text_rect.width();
        let height = text_rect.height();
        let child_size = self
            .text_layout
            .with_untracked(|text_layout| text_layout.as_ref().unwrap().size());

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
    fn id(&self) -> ViewId {
        self.id
    }

    fn update(
        &mut self,
        _cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) {
        if let Ok(state) = state.downcast() {
            match *state {
                TextInputState::Content {
                    text,
                    offset,
                    preedit_range,
                } => {
                    self.content = text;
                    self.offset = offset;
                    self.preedit_range = preedit_range;
                    self.text_layout.set(None);
                }
                TextInputState::Focus(focus) => {
                    self.focus = focus;
                }
                TextInputState::Placeholder(placeholder) => {
                    self.placeholder = placeholder;
                    self.placeholder_text_layout = None;
                }
            }
            self.id.request_layout();
        }
    }

    fn style(&mut self, cx: &mut floem::context::StyleCx<'_>) {
        if self.style.read(cx) {
            self.set_text_layout();
            self.id.request_layout();
        }
    }

    fn layout(
        &mut self,
        cx: &mut floem::context::LayoutCx,
    ) -> floem::taffy::prelude::NodeId {
        cx.layout_node(self.id, true, |_cx| {
            if self
                .text_layout
                .with_untracked(|text_layout| text_layout.is_none())
                || self.placeholder_text_layout.is_none()
            {
                self.set_text_layout();
            }

            let text_layout = self.text_layout;
            text_layout.with_untracked(|text_layout| {
                let text_layout = text_layout.as_ref().unwrap();

                let offset = self.cursor().get_untracked().offset();
                let cursor_point = text_layout.hit_position(offset).point;
                if cursor_point != self.cursor_pos {
                    self.cursor_pos = cursor_point;
                    self.ensure_cursor_visible();
                }

                let size = text_layout.size();
                let height = size.height as f32;

                if self.text_node.is_none() {
                    self.text_node = Some(self.id.new_taffy_node());
                }

                let text_node = self.text_node.unwrap();

                let style = Style::new().height(height).to_taffy_style();
                self.id.set_taffy_style(text_node, style);
            });

            vec![self.text_node.unwrap()]
        })
    }

    fn compute_layout(
        &mut self,
        _cx: &mut floem::context::ComputeLayoutCx,
    ) -> Option<Rect> {
        let layout = self.id.get_layout().unwrap_or_default();

        let style = self.id.get_combined_style();
        let style = style.builtin();
        let padding_left = match style.padding_left() {
            PxPct::Px(padding) => padding,
            PxPct::Pct(pct) => pct * layout.size.width as f64,
        };
        let padding_right = match style.padding_right() {
            PxPct::Px(padding) => padding,
            PxPct::Pct(pct) => pct * layout.size.width as f64,
        };

        let size = Size::new(layout.size.width as f64, layout.size.height as f64);
        let mut text_rect = size.to_rect();
        text_rect.x0 += padding_left;
        text_rect.x1 -= padding_right;
        self.text_rect = text_rect;

        self.clamp_text_viewport(self.text_viewport);

        let text_node = self.text_node.unwrap();
        let location = self.id.taffy_layout(text_node).unwrap_or_default().location;
        self.layout_rect = size
            .to_rect()
            .with_origin(Point::new(location.x as f64, location.y as f64));
        let offset = self.cursor().with_untracked(|c| c.offset());
        let cursor_line = self.text_layout.with_untracked(|text_layout| {
            let hit_position = text_layout.as_ref().unwrap().hit_position(offset);
            let point = Point::new(location.x as f64, location.y as f64)
                - self.text_viewport.origin().to_vec2();
            let cursor_point = hit_position.point + point.to_vec2();

            Line::new(
                Point::new(
                    cursor_point.x,
                    cursor_point.y - hit_position.glyph_ascent,
                ),
                Point::new(
                    cursor_point.x,
                    cursor_point.y + hit_position.glyph_descent,
                ),
            )
        });
        self.cursor_line.set(cursor_line);

        None
    }

    fn event_before_children(
        &mut self,
        cx: &mut floem::context::EventCx,
        event: &floem::event::Event,
    ) -> EventPropagation {
        let text_offset = self.text_viewport.origin();
        let event = event.clone().offset((-text_offset.x, -text_offset.y));
        match event {
            Event::PointerDown(pointer) => {
                let offset = self.hit_index(cx, pointer.pos);
                self.cursor().update(|cursor| {
                    cursor.set_insert(Selection::caret(offset));
                });
                if pointer.button.is_primary() && pointer.count == 2 {
                    let offset = self.hit_index(cx, pointer.pos);
                    let (start, end) = self
                        .doc()
                        .buffer
                        .with_untracked(|buffer| buffer.select_word(offset));
                    self.cursor().update(|cursor| {
                        cursor.set_insert(Selection::region(start, end));
                    });
                } else if pointer.button.is_primary() && pointer.count == 3 {
                    self.cursor().update(|cursor| {
                        cursor.set_insert(Selection::region(0, self.content.len()));
                    });
                }
                cx.update_active(self.id);
            }
            Event::PointerMove(pointer) => {
                if cx.is_active(self.id) {
                    let offset = self.hit_index(cx, pointer.pos);
                    self.cursor().update(|cursor| {
                        cursor.set_offset(offset, true, false);
                    });
                }
            }
            Event::PointerWheel(pointer_event) => {
                let delta = pointer_event.delta;
                let delta = if delta.x == 0.0 && delta.y != 0.0 {
                    Vec2::new(delta.y, delta.x)
                } else {
                    delta
                };
                self.clamp_text_viewport(self.text_viewport + delta);
                return EventPropagation::Continue;
            }
            _ => {}
        }
        EventPropagation::Continue
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        cx.save();
        cx.clip(&self.text_rect.inflate(1.0, 0.0));
        let text_node = self.text_node.unwrap();
        let location = self.id.taffy_layout(text_node).unwrap_or_default().location;
        let point = Point::new(location.x as f64, location.y as f64)
            - self.text_viewport.origin().to_vec2();

        self.text_layout.with_untracked(|text_layout| {
            let text_layout = text_layout.as_ref().unwrap();
            let height = text_layout.size().height;
            let config = self.config.get_untracked();

            let cursor = self.cursor().get_untracked();

            if let CursorMode::Insert(selection) = &cursor.mode {
                for region in selection.regions() {
                    if !region.is_caret() {
                        let min = text_layout.hit_position(region.min()).point.x;
                        let max = text_layout.hit_position(region.max()).point.x;
                        cx.fill(
                            &Rect::ZERO
                                .with_size(Size::new(max - min, height))
                                .with_origin(Point::new(min + point.x, point.y)),
                            config.color(LapceColor::EDITOR_SELECTION),
                            0.0,
                        );
                    }
                }
            }

            if !self.content.is_empty() {
                cx.draw_text(text_layout, point);
            } else if !self.placeholder.is_empty() {
                cx.draw_text(self.placeholder_text_layout.as_ref().unwrap(), point);
            }

            if let Some((start, end)) = self.preedit_range {
                let start_position = text_layout.hit_position(start);
                let start_point = start_position.point
                    + self.layout_rect.origin().to_vec2()
                    - self.text_viewport.origin().to_vec2();
                let end_position = text_layout.hit_position(end);
                let end_point = end_position.point
                    + self.layout_rect.origin().to_vec2()
                    - self.text_viewport.origin().to_vec2();

                let line = Line::new(
                    Point::new(
                        start_point.x,
                        start_point.y + start_position.glyph_descent,
                    ),
                    Point::new(
                        end_point.x,
                        end_point.y + end_position.glyph_descent,
                    ),
                );
                cx.stroke(&line, config.color(LapceColor::EDITOR_FOREGROUND), 1.0);
            }

            if !self.hide_cursor.get_untracked()
                && (self.focus || cx.is_focused(self.id))
            {
                cx.clip(&self.text_rect.inflate(2.0, 2.0));

                let hit_position = text_layout.hit_position(self.offset);
                let cursor_point = hit_position.point
                    + self.layout_rect.origin().to_vec2()
                    - self.text_viewport.origin().to_vec2();

                let line = Line::new(
                    Point::new(
                        cursor_point.x,
                        cursor_point.y - hit_position.glyph_ascent,
                    ),
                    Point::new(
                        cursor_point.x,
                        cursor_point.y + hit_position.glyph_descent,
                    ),
                );

                cx.stroke(
                    &line,
                    self.config.get_untracked().color(LapceColor::EDITOR_CARET),
                    2.0,
                );
            }

            cx.restore();
        });
    }
}
