use floem::{
    app::AppContext,
    id::Id,
    parley::{
        layout::{Alignment, Cursor},
        style::StyleProperty,
        swash::Weight,
        LayoutContext,
    },
    peniko::{kurbo::Point, Brush, Color},
    reactive::create_effect,
    style::{Dimension, Style},
    taffy::prelude::Node,
    text::ParleyBrush,
    view::{ChangeFlags, View},
};

enum FocusTextState {
    Text(String),
    FocusColor(Color),
    FocusIndices(Vec<usize>),
}

pub fn focus_text(
    cx: AppContext,
    text: impl Fn() -> String + 'static,
    focus_indices: impl Fn() -> Vec<usize> + 'static,
    focus_color: impl Fn() -> Color + 'static,
) -> FocusText {
    let id = cx.new_id();

    create_effect(cx.scope, move |_| {
        let new_text = text();
        AppContext::update_state(id, FocusTextState::Text(new_text));
    });

    create_effect(cx.scope, move |_| {
        let focus_color = focus_color();
        AppContext::update_state(id, FocusTextState::FocusColor(focus_color));
    });

    create_effect(cx.scope, move |_| {
        let focus_indices = focus_indices();
        AppContext::update_state(id, FocusTextState::FocusIndices(focus_indices));
    });

    FocusText {
        id,
        text: "".to_string(),
        text_layout: None,
        color: None,
        focus_color: Color::default(),
        focus_indices: Vec::new(),
        text_node: None,
        font_size: None,
        available_text: None,
        available_width: None,
        available_text_layout: None,
    }
}

pub struct FocusText {
    id: Id,
    text: String,
    text_layout: Option<floem::parley::Layout<ParleyBrush>>,
    color: Option<Color>,
    focus_color: Color,
    focus_indices: Vec<usize>,
    font_size: Option<f32>,
    text_node: Option<Node>,
    available_text: Option<String>,
    available_width: Option<f32>,
    available_text_layout: Option<floem::parley::Layout<ParleyBrush>>,
}

impl FocusText {
    fn set_text_layout(&mut self) {
        let mut text_layout_builder =
            LayoutContext::builder(self.text.as_str(), 1.0);
        text_layout_builder.push_default(&StyleProperty::Brush(ParleyBrush(
            Brush::Solid(self.color.unwrap_or_default()),
        )));
        if let Some(font_size) = self.font_size {
            text_layout_builder.push_default(&StyleProperty::FontSize(font_size));
        }
        for &i_start in &self.focus_indices {
            let i_end = self
                .text
                .char_indices()
                .find(|(i, _)| *i == i_start)
                .map(|(_, c)| c.len_utf8() + i_start);
            let i_end = if let Some(i_end) = i_end {
                i_end
            } else {
                continue;
            };
            text_layout_builder.push(
                &StyleProperty::Brush(ParleyBrush(Brush::Solid(self.focus_color))),
                i_start..i_end,
            );
            text_layout_builder
                .push(&StyleProperty::FontWeight(Weight::BOLD), i_start..i_end);
        }
        let mut text_layout = text_layout_builder.build();
        text_layout.break_all_lines(None, Alignment::Start);
        self.text_layout = Some(text_layout);

        if let Some(new_text) = self.available_text.as_ref() {
            let new_text_len = new_text.len();

            let mut text_layout_builder =
                LayoutContext::builder(new_text.as_str(), 1.0);
            text_layout_builder.push_default(&StyleProperty::Brush(ParleyBrush(
                Brush::Solid(self.color.unwrap_or_default()),
            )));
            if let Some(font_size) = self.font_size {
                text_layout_builder
                    .push_default(&StyleProperty::FontSize(font_size));
            }
            for &i_start in &self.focus_indices {
                if i_start + 3 > new_text_len {
                    break;
                }
                let i_end = self
                    .text
                    .char_indices()
                    .find(|(i, _)| *i == i_start)
                    .map(|(_, c)| c.len_utf8() + i_start);
                let i_end = if let Some(i_end) = i_end {
                    i_end
                } else {
                    continue;
                };
                text_layout_builder.push(
                    &StyleProperty::Brush(ParleyBrush(Brush::Solid(
                        self.focus_color,
                    ))),
                    i_start..i_end,
                );
                text_layout_builder
                    .push(&StyleProperty::FontWeight(Weight::BOLD), i_start..i_end);
            }
            let mut new_text = text_layout_builder.build();
            new_text.break_all_lines(None, Alignment::Start);
            self.available_text_layout = Some(new_text);
        }
    }
}

impl View for FocusText {
    fn id(&self) -> floem::id::Id {
        self.id
    }

    fn child(&mut self, id: floem::id::Id) -> Option<&mut dyn View> {
        None
    }

    fn update(
        &mut self,
        cx: &mut floem::context::UpdateCx,
        state: Box<dyn std::any::Any>,
    ) -> floem::view::ChangeFlags {
        if let Ok(state) = state.downcast() {
            match *state {
                FocusTextState::Text(text) => {
                    self.text = text;
                }
                FocusTextState::FocusColor(color) => {
                    self.focus_color = color;
                }
                FocusTextState::FocusIndices(indices) => {
                    self.focus_indices = indices;
                }
            }
            self.set_text_layout();
            cx.request_layout(self.id());
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
            if self.font_size != cx.current_font_size() {
                self.font_size = cx.current_font_size();
                self.set_text_layout();
            }
            if self.text_layout.is_none() {
                self.set_text_layout();
            }
            let text_layout = self.text_layout.as_ref().unwrap();
            let width = text_layout.width().ceil();
            let height = text_layout.height().ceil();

            if self.text_node.is_none() {
                self.text_node = Some(cx.new_node());
            }
            let text_node = self.text_node.unwrap();

            cx.set_style(
                text_node,
                (&Style {
                    width: Dimension::Points(width),
                    height: Dimension::Points(height),
                    ..Default::default()
                })
                    .into(),
            );
            vec![text_node]
        })
    }

    fn compute_layout(&mut self, cx: &mut floem::context::LayoutCx) {
        let text_node = self.text_node.unwrap();
        let layout = cx.layout(text_node).unwrap();
        let text_layout = self.text_layout.as_ref().unwrap();
        let width = text_layout.width();
        if width > layout.size.width {
            if self.available_width != Some(layout.size.width) {
                let mut text_layout_builder = LayoutContext::builder("...", 1.0);
                text_layout_builder.push_default(&StyleProperty::Brush(
                    ParleyBrush(Brush::Solid(Color::rgb8(0xf0, 0xf0, 0xea))),
                ));
                if let Some(font_size) = self.font_size {
                    text_layout_builder
                        .push_default(&StyleProperty::FontSize(font_size));
                }
                let mut dots_text = text_layout_builder.build();
                dots_text.break_all_lines(None, Alignment::Start);
                let dots_width = dots_text.width();
                let width_left = layout.size.width - dots_width;
                let cursor = Cursor::from_point(text_layout, width_left, 0.0);
                let range = cursor.text_range();
                let index = if cursor.is_trailing() {
                    range.end
                } else {
                    range.start
                };

                let new_text = if index > 0 {
                    format!("{}...", &self.text[..index])
                } else {
                    "".to_string()
                };
                self.available_text = Some(new_text);
                self.available_width = Some(layout.size.width);
                self.set_text_layout();
            }
        } else {
            self.available_text = None;
            self.available_width = None;
            self.available_text_layout = None;
        }
    }

    fn event(
        &mut self,
        cx: &mut floem::context::EventCx,
        id_path: Option<&[floem::id::Id]>,
        event: floem::event::Event,
    ) -> bool {
        false
    }

    fn paint(&mut self, cx: &mut floem::context::PaintCx) {
        if self.color != cx.current_color()
            || self.font_size != cx.current_font_size()
        {
            self.color = cx.current_color();
            self.font_size = cx.current_font_size();
            self.set_text_layout();
        }
        let text_node = self.text_node.unwrap();
        let location = cx.layout(text_node).unwrap().location;
        let point = Point::new(location.x as f64, location.y as f64);
        if let Some(text_layout) = self.available_text_layout.as_ref() {
            cx.render_text(text_layout, point);
        } else {
            cx.render_text(self.text_layout.as_ref().unwrap(), point);
        }
    }
}
