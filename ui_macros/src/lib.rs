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
        impl Widget for #name {
            fn id(&self) -> String {
                self.state.lock().unwrap().id().clone()
            }

            fn set_window_handle(&self, handle: WindowHandle) {
                self.state.lock().unwrap().set_window_handle(handle);
            }

            fn invalidate(&self) {
                self.state.lock().unwrap().invalidate();
            }

            fn invalidate_rect(&self, rect: Rect) {
                self.state.lock().unwrap().invalidate_rect(rect);
            }

            fn set_rect(&self, rect: Rect)  {
                println!("set rect {:?}", rect);
                self.state.lock().unwrap().set_rect(rect);
                self.layout();
            }

            fn set_active(&self) {
                self.state.lock().unwrap().set_active();
            }

            fn get_rect(&self) -> Rect {
                self.state.lock().unwrap().get_rect()
            }

            fn paint_raw(&self, paint_ctx: &mut PaintCtx, rect: Rect) {
                let layout_rect = self.state.lock().unwrap().get_rect();
                let rect = layout_rect.intersect(rect);
                if rect.area() == 0.0 {
                    return;
                }

                paint_ctx.save();
                paint_ctx.clip(rect);
                paint_ctx.transform(Affine::translate(layout_rect.origin().to_vec2()));
                self.paint(paint_ctx);
                let new_rect = rect - layout_rect.origin().to_vec2() ;
                self.state.lock().unwrap().paint_children(paint_ctx, new_rect.clone());
                paint_ctx.restore();

            }

            fn size(&self, width: f64, height: f64) {
                let rect = self.state.lock().unwrap().get_rect().with_size(Size::new(width, height));
                self.set_rect(rect);
            }

            fn set_parent(&self, parent: Box<Widget>) {
                self.state.lock().unwrap().set_parent(parent);
            }

            fn add_child(&self, child: Box<Widget>) {
                child.set_parent(Box::new(self.clone()));
                self.state.lock().unwrap().add_child(child);
                self.layout();
            }

            fn contains(&self, pos: Point) -> bool {
                self.state.lock().unwrap().contains(pos)
            }

            fn mouse_down_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) {
                let rect = self.state.lock().unwrap().get_rect();
                let mut event = event.clone();
                event.pos = event.pos - rect.origin().to_vec2();

                if !self.state.lock().unwrap().child_mouse_down(&event, ctx) {
                    self.state.lock().unwrap().set_active();
                    self.mouse_down(&event, ctx);
                } else {
                    self.state.lock().unwrap().set_inactive();
                }
            }

            fn mouse_move_raw(&self, event: &MouseEvent, ctx: &mut dyn WinCtx) -> bool {
                let rect = self.state.lock().unwrap().get_rect();
                let mut child_event = event.clone();
                child_event.pos = event.pos - rect.origin().to_vec2();

                let in_children = self.state.lock().unwrap().child_mouse_move(&child_event, ctx);
                if in_children {
                    self.state.lock().unwrap().no_focus();
                    return true;
                }
                if self.state.lock().unwrap().contains(event.pos) {
                    self.state.lock().unwrap().set_focus();
                    return true;
                }
                self.state.lock().unwrap().no_focus();
                false
            }

            fn wheel_raw(&self, delta: Vec2, mods: KeyModifiers, ctx: &mut dyn WinCtx) {
                let is_focus = self.state.lock().unwrap().is_focus();
                if is_focus {
                    self.wheel(delta, mods, ctx);
                    return;
                }
                self.state.lock().unwrap().child_wheel(delta, mods, ctx);
            }

            fn key_down_raw(&self, event: KeyEvent, ctx: &mut dyn WinCtx) {
                let is_active = self.state.lock().unwrap().is_active();
                if is_active {
                    self.key_down(event, ctx);
                    return;
                }
                self.state.lock().unwrap().child_key_down(event, ctx);
            }

        }
    };

    TokenStream::from(expanded)
}
