extern crate proc_macro;

use crate::proc_macro::TokenStream;

use quote::quote;

use syn::DeriveInput;

#[proc_macro_derive(WidgetBase)]
pub fn widget_base_derive(input: TokenStream) -> TokenStream {
    let ast: DeriveInput = syn::parse(input).unwrap();

    // get the name of the type we want to implement the trait for
    let name = &ast.ident;

    let expanded = quote! {
        impl WidgetTrait for #name {
            fn id(&self) -> String {
                self.widget_state.lock().unwrap().id().clone()
            }

            fn set_window_handle(&self, handle: WindowHandle) {
                self.widget_state.lock().unwrap().set_window_handle(handle);
            }

            fn invalidate(&self) {
                self.widget_state.lock().unwrap().invalidate();
            }

            fn layout_raw(&self) {
                self.layout();
            }

            fn set_size(&self, width: f64, height: f64) {
                self.widget_state.lock().unwrap().set_size(width, height);
            }

            fn set_content_size(&self, width: f64, height: f64) {
                self.widget_state.lock().unwrap().set_content_size(width, height);
            }

            fn set_pos(&self, x: f64, y: f64) {
                self.widget_state.lock().unwrap().set_pos(x, y);
            }

            fn set_padding(&self, top: f64, right: f64, bottom: f64, left: f64) {
                self.widget_state.lock().unwrap().set_padding(top, right, bottom, left);
            }

            fn set_background(&self, color: Color) {
                self.widget_state.lock().unwrap().set_background(color);
            }

            fn background(&self) -> Option<Color> {
                self.widget_state.lock().unwrap().background()
            }

            fn padding(&self) -> (f64, f64, f64, f64) {
                self.widget_state.lock().unwrap().padding()
            }

            fn set_shadow(&self, horizontal: f64, vertical: f64, blur: f64, spread: f64, color: Color) {
                self.widget_state.lock().unwrap().set_shadow(horizontal, vertical, blur, spread, color)
            }

            fn shadow(&self) -> (f64, f64, f64, f64, Option<Color>) {
                self.widget_state.lock().unwrap().shadow()
            }

            fn invalidate_rect(&self, rect: Rect) {
                self.widget_state.lock().unwrap().invalidate_rect(rect);
            }

            fn set_rect(&self, rect: Rect)  {
                self.widget_state.lock().unwrap().set_rect(rect);
                self.layout();
            }

            fn show(&self) {
                self.widget_state.lock().unwrap().show();
            }

            fn hide(&self) {
                self.widget_state.lock().unwrap().hide();
            }

            fn is_hidden(&self) -> bool {
                self.widget_state.lock().unwrap().is_hidden()
            }

            fn set_custom_rect(&self, rect: Rect)  {
                self.widget_state.lock().unwrap().set_custom_rect(rect);
            }

            fn parent(&self) -> Option<Box<WidgetTrait>> {
                self.widget_state.lock().unwrap().parent()
            }

            fn set_active(&self) {
                let top_parent = self.widget_state.lock().unwrap().top_parent();
                match top_parent {
                    Some(parent)=>parent.set_inactive(true),
                    None => (),
                }
                self.widget_state.lock().unwrap().set_active();
            }

            fn set_inactive(&self, propagate: bool) {
                self.widget_state.lock().unwrap().set_inactive(propagate);
            }

            fn get_rect(&self) -> Rect {
                self.widget_state.lock().unwrap().get_rect()
            }

            fn custom_rect(&self) -> Rect {
                self.widget_state.lock().unwrap().custom_rect()
            }

            fn paint_raw(&self, paint_ctx: &mut PaintCtx, paint_rect: Rect) {
                if self.widget_state.lock().unwrap().is_hidden() {
                    return;
                }

                let widget_rect = self.get_rect();
                if widget_rect.intersect(paint_rect).area() == 0.0 {
                    return;
                }

                self.widget_state.lock().unwrap().paint(paint_ctx);

                let (top, right, bottom, left) = self.padding();
                let layout_rect = Rect::new(widget_rect.x0 + left, widget_rect.y0 +top, widget_rect.x1 - right, widget_rect.y1 - bottom);
                let paint_rect = layout_rect.intersect(paint_rect);
                if paint_rect.area() == 0.0 {
                    return;
                }

                let horizontal_scroll = self.widget_state.lock().unwrap().horizontal_scroll();
                let vertical_scroll = self.widget_state.lock().unwrap().vertical_scroll();
                paint_ctx.save();
                paint_ctx.clip(paint_rect);
                let content_size = self.widget_state.lock().unwrap().content_size();
                let vec2 = if content_size.width == 0.0 && content_size.height == 0.0 {
                    layout_rect.origin().to_vec2()
                } else {
                    layout_rect.origin().to_vec2() - Vec2::new(horizontal_scroll, vertical_scroll)
                };
                paint_ctx.transform(Affine::translate(vec2));
                self.paint(paint_ctx);
                paint_ctx.restore();

                paint_ctx.save();
                paint_ctx.clip(paint_rect);
                paint_ctx.transform(Affine::translate(layout_rect.origin().to_vec2()));
                let new_paint_rect = paint_rect - layout_rect.origin().to_vec2() ;
                self.widget_state.lock().unwrap().paint_children(paint_ctx, new_paint_rect.clone());
                paint_ctx.restore();

            }

            fn size(&self, width: f64, height: f64) {
                let rect = self.widget_state.lock().unwrap().get_rect().with_size(Size::new(width, height));
                self.set_rect(rect);
            }

            fn set_parent(&self, parent: Box<WidgetTrait>) {
                self.widget_state.lock().unwrap().set_parent(parent);
            }

            fn horizontal_scroll(&self) -> f64 {
                self.widget_state.lock().unwrap().horizontal_scroll()
            }

            fn vertical_scroll(&self) -> f64 {
                self.widget_state.lock().unwrap().vertical_scroll()
            }

            fn add_child(&self, child: Box<WidgetTrait>) {
                child.set_parent(Box::new(self.clone()));
                self.widget_state.lock().unwrap().add_child(child);
                self.layout();
            }

            fn replace_child(&self, index: usize, child: Box<WidgetTrait>) {
                child.set_parent(Box::new(self.clone()));
                self.widget_state.lock().unwrap().replace_child(index, child);
                self.layout();
            }

            fn contains(&self, pos: Point) -> bool {
                self.widget_state.lock().unwrap().contains(pos)
            }

            fn mouse_down_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool {
                if self.is_hidden() {
                    self.widget_state.lock().unwrap().set_inactive(false);
                    return false
                }

                let rect = self.widget_state.lock().unwrap().get_rect();
                let mut child_event = event.clone();
                child_event.pos = event.pos - rect.origin().to_vec2();

                let in_children = self.widget_state.lock().unwrap().child_mouse_down(&child_event, ctx);
                if in_children {
                    self.widget_state.lock().unwrap().set_inactive(false);
                    return true;
                }
                if self.widget_state.lock().unwrap().contains(event.pos) {
                    self.widget_state.lock().unwrap().set_active();
                    self.mouse_down(&child_event, ctx);
                    return true;
                }
                self.widget_state.lock().unwrap().set_inactive(false);
                false
            }

            fn mouse_move_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool {
                let rect = self.widget_state.lock().unwrap().get_rect();
                let mut child_event = event.clone();
                child_event.pos = event.pos - rect.origin().to_vec2();

                let in_children = self.widget_state.lock().unwrap().child_mouse_move(&child_event, ctx);
                if in_children {
                    self.widget_state.lock().unwrap().no_focus();
                    return true;
                }
                if self.widget_state.lock().unwrap().contains(event.pos) {
                    self.widget_state.lock().unwrap().set_focus();
                    return true;
                }
                self.widget_state.lock().unwrap().no_focus();
                false
            }

            fn wheel_raw(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
                let is_focus = self.widget_state.lock().unwrap().is_focus();
                if is_focus {
                    self.wheel(delta, mods, ctx);
                    return;
                }
                self.widget_state.lock().unwrap().child_wheel(delta, mods, ctx);
            }

            fn key_down_raw(&self, event: KeyEvent, ctx: &mut dyn WinCtx) -> bool {
                if self.key_down(event, ctx) {
                    return true;
                }
                return self.widget_state.lock().unwrap().child_key_down(event, ctx);
            }

            fn child_ids(&self) -> Vec<String> {
                self.widget_state.lock().unwrap().child_ids()
            }

            fn set_scroll(&self, horizontal: f64, vertical: f64) {
                self.widget_state.lock().unwrap().set_scroll(horizontal, vertical);
            }

            fn ensure_visble(&self, rect: Rect, margin_x: f64, margin_y: f64) {
                self.widget_state.lock().unwrap().ensure_visble(rect, margin_x, margin_y);
            }

        }
    };

    TokenStream::from(expanded)
}
