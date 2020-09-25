use crate::app::{App, CommandRunner};
use crate::input::{Cmd, Command, Input, InputState, KeyInput};
use druid::shell::keyboard::{KeyEvent, KeyModifiers};
use druid::shell::window::{MouseEvent, WinCtx, WinHandler, WindowHandle};
use druid::PaintCtx;
use druid::{kurbo, piet};
use fzyr::{has_match, locate, Score};
use kurbo::{Affine, Point, Rect, Size, Vec2};
use lsp_types::{CompletionItem, CompletionResponse};
use piet::{
    Color, FontBuilder, LinearGradient, RenderContext, Text, TextLayout,
    TextLayoutBuilder, UnitPoint,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct PopupItem {
    label: String,
    score: Score,
}

pub struct PopupState {
    items: Vec<PopupItem>,
    filtered_items: Vec<PopupItem>,
    col: usize,
    line: usize,
    filter: String,
    index: usize,
}

#[derive(Clone, WidgetBase)]
pub struct Popup {
    widget_state: Arc<Mutex<WidgetState>>,
    state: Arc<Mutex<PopupState>>,
    app: Box<App>,
}

impl CommandRunner for Popup {
    fn run(&self, cmd: Cmd, key_input: KeyInput) {
        match cmd.clone().cmd.unwrap() {
            Command::MoveDown => self.change_index(1),
            Command::MoveUp => self.change_index(-1),
            Command::Execute => {
                let items = self.items();
                let index = self.state.lock().unwrap().index;
                let filter = self.state.lock().unwrap().filter.clone();
                let view = self.app.get_active_editor().view();
                if filter != "" {
                    self.app.core.send_notification(
                        "edit",
                        &json!({
                            "view_id": view.id(),
                            "method": "delete_word_backward",
                        }),
                    );
                }
                self.app.core.send_notification(
                    "edit",
                    &json!({
                        "view_id": view.id(),
                        "method": "insert",
                        "params": {"chars": items.get(index).unwrap().label.clone()},
                    }),
                );
                self.cancel();
                self.hide();
                self.invalidate();
            }
            _ => (),
        }
    }
}

impl PopupState {
    pub fn new() -> PopupState {
        PopupState {
            items: Vec::new(),
            filtered_items: Vec::new(),
            line: 0,
            col: 0,
            filter: "".to_string(),
            index: 0,
        }
    }
}

impl Popup {
    pub fn new(app: App) -> Popup {
        Popup {
            widget_state: Arc::new(Mutex::new(WidgetState::new())),
            state: Arc::new(Mutex::new(PopupState::new())),
            app: Box::new(app.clone()),
        }
    }

    fn change_index(&self, n: i64) {
        let app_font = self.app.config.font.lock().unwrap().clone();
        let size = self.get_rect().size();
        let items = self.items();
        let index = self.state.lock().unwrap().index;

        let new_index = if index as i64 + n < 0 {
            items.len() as i64 + index as i64 + n
        } else if index as i64 + n > items.len() as i64 - 1 {
            index as i64 + n - items.len() as i64
        } else {
            index as i64 + n
        } as usize;

        self.state.lock().unwrap().index = new_index;
        self.ensure_visble(
            Rect::from_origin_size(
                Point::new(0.0, new_index as f64 * app_font.lineheight()),
                Size::new(size.width, app_font.lineheight()),
            ),
            0.0,
            0.0,
        );
        self.invalidate();
    }

    pub fn cancel(&self) {
        self.state.lock().unwrap().filter = "".to_string();
        self.state.lock().unwrap().index = 0;
    }

    fn update_height(&self) {
        let height = {
            let items = self.items();
            let size = self.get_rect().size();
            let lines = if items.len() > 10 { 10 } else { items.len() };
            let app_font = self.app.config.font.lock().unwrap().clone();
            self.set_content_size(
                size.width,
                items.len() as f64 * app_font.lineheight(),
            );
            lines as f64 * app_font.lineheight()
        };
        let size = self.get_rect().size();
        if height != size.height {
            self.invalidate();
            self.set_size(size.width, height);
        }
    }

    pub fn set_completion(&self, completion: CompletionResponse) {
        let items: Vec<PopupItem> = match completion {
            CompletionResponse::Array(completion_items) => completion_items,
            CompletionResponse::List(completion_list) => completion_list.items,
        }
        .iter()
        .map(|item| PopupItem {
            score: 0.0,
            label: item.label.clone(),
        })
        .collect();

        let (_col, _line, filter) =
            self.app.get_active_editor().get_completion_pos();
        self.state.lock().unwrap().items = items;
        self.filter_items(filter);
    }

    pub fn items(&self) -> Vec<PopupItem> {
        let filter = self.state.lock().unwrap().filter.clone();
        match filter.as_ref() {
            "" => self.state.lock().unwrap().items.clone(),
            _ => self.state.lock().unwrap().filtered_items.clone(),
        }
    }

    pub fn filter_items(&self, filter: String) {
        println!("filter is '{}'", filter);
        self.state.lock().unwrap().filter = filter.clone();
        self.state.lock().unwrap().index = 0;
        if filter == "" {
            self.update_height();
            return;
        }
        let mut filtered_items: Vec<PopupItem> = Vec::new();
        for item in &self.state.lock().unwrap().items {
            if has_match(&filter, &item.label) {
                let result = locate(&filter, &item.label);
                let mut filtered_item = item.clone();
                filtered_item.score = result.score;
                let index =
                    match filtered_items.binary_search_by(|other_item| {
                        filtered_item
                            .score
                            .partial_cmp(&other_item.score)
                            .unwrap()
                    }) {
                        Ok(index) => index,
                        Err(index) => index,
                    };
                filtered_items.insert(index, filtered_item);
            }
        }
        self.state.lock().unwrap().filtered_items = filtered_items;
        self.update_height();
    }

    pub fn set_location(&self, col: usize, line: usize) {
        self.state.lock().unwrap().col = col;
        self.state.lock().unwrap().line = line;
    }

    pub fn line(&self) -> usize {
        self.state.lock().unwrap().line
    }

    pub fn col(&self) -> usize {
        self.state.lock().unwrap().col
    }

    fn layout(&self) {}

    fn paint(&self, paint_ctx: &mut PaintCtx) {
        let app_font = self.app.config.font.lock().unwrap().clone();
        let size = self.get_rect().size();
        let fg = self.app.config.theme.lock().unwrap().foreground.unwrap();

        let index = self.state.lock().unwrap().index;
        paint_ctx.fill(
            Rect::from_origin_size(
                Point::new(0.0, app_font.lineheight() * index as f64),
                Size::new(size.width, app_font.lineheight()),
            ),
            &Color::rgba8(fg.r, fg.g, fg.b, 20),
        );

        let font = paint_ctx
            .text()
            .new_font_by_name("Cascadia Code", 13.0)
            .unwrap()
            .build()
            .unwrap();
        let mut i = 0;
        for item in self.items() {
            let layout = paint_ctx
                .text()
                .new_text_layout(&font, &item.label)
                .unwrap()
                .build()
                .unwrap();
            paint_ctx.draw_text(
                &layout,
                Point::new(
                    0.0,
                    app_font.ascent
                        + app_font.linespace / 2.0
                        + i as f64 * app_font.lineheight(),
                ),
                &Color::rgba8(fg.r, fg.g, fg.b, 255),
            );
            i += 1;
        }
    }

    fn mouse_down(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {}

    fn wheel(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {}

    fn key_down(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
        false
    }
}
