//! This button code is adapted from druid's button widget

use druid::{
    debug_state::DebugState,
    theme,
    widget::{Click, ControllerHost, Label, LabelText},
    Affine, BoxConstraints, Cursor, Data, Env, Event, EventCtx, FontDescriptor,
    Insets, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx, RenderContext, Size,
    UpdateCtx, Widget,
};
use lapce_data::{
    config::{LapceConfig, LapceTheme},
    data::LapceTabData,
};

// the minimum padding added to a button.
// NOTE: these values are chosen to match the existing look of TextBox; these
// should be reevaluated at some point.
const LABEL_INSETS: Insets = Insets::uniform_xy(8., 2.);

/// A button with a text label.
pub struct Button {
    label: Label<LapceTabData>,
    label_size: Size,
}

impl Button {
    /// Create a new button with a text label.
    pub fn new(
        data: &LapceTabData,
        text: impl Into<LabelText<LapceTabData>>,
    ) -> Button {
        Button::from_label(data, Label::new(text))
    }

    /// Create a new button with the provided [`Label`].
    pub fn from_label(
        data: &LapceTabData,
        mut label: Label<LapceTabData>,
    ) -> Button {
        update_label(&data.config, &mut label);
        Button {
            label,
            label_size: Size::ZERO,
        }
    }

    /// Provide a closure to be called when this button is clicked.
    pub fn on_click(
        self,
        f: impl Fn(&mut EventCtx, &mut LapceTabData, &Env) + 'static,
    ) -> ControllerHost<Self, Click<LapceTabData>> {
        ControllerHost::new(self, Click::new(f))
    }
}

impl Widget<LapceTabData> for Button {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        _data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseDown(_) => {
                if !ctx.is_disabled() {
                    ctx.set_active(true);
                    ctx.request_paint();
                }
            }
            Event::MouseMove(_) => ctx.set_cursor(&Cursor::Pointer),
            Event::MouseUp(_) => {
                if ctx.is_active() && !ctx.is_disabled() {
                    ctx.request_paint();
                }
                ctx.set_active(false);
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        if let LifeCycle::HotChanged(_) | LifeCycle::DisabledChanged(_) = event {
            ctx.request_paint();
        }
        self.label.lifecycle(ctx, event, data, env)
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if !old_data.config.same(&data.config) {
            update_label(&data.config, &mut self.label);
        }
        self.label.update(ctx, old_data, data, env)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        let padding = Size::new(LABEL_INSETS.x_value(), LABEL_INSETS.y_value());
        let label_bc = bc.shrink(padding).loosen();
        self.label_size = self.label.layout(ctx, &label_bc, data, env);
        // HACK: to make sure we look okay at default sizes when beside a textbox,
        // we make sure we will have at least the same height as the default textbox.
        let min_height = env.get(theme::BORDERED_WIDGET_HEIGHT);
        let baseline = self.label.baseline_offset();
        ctx.set_baseline_offset(baseline + LABEL_INSETS.y1);

        bc.constrain(Size::new(
            self.label_size.width + padding.width,
            (self.label_size.height + padding.height).max(min_height),
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        let size = ctx.size();
        let stroke_width = 1.0;

        let rounded_rect = size
            .to_rect()
            .inset(-stroke_width / 2.0)
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));

        ctx.stroke(
            rounded_rect,
            data.config.get_color_unchecked(LapceTheme::LAPCE_BORDER),
            stroke_width,
        );

        let label_offset = (size.to_vec2() - self.label_size.to_vec2()) / 2.0;

        ctx.with_save(|ctx| {
            ctx.transform(Affine::translate(label_offset));
            self.label.paint(ctx, data, env);
        });
    }

    fn debug_state(&self, _data: &LapceTabData) -> DebugState {
        DebugState {
            display_name: self.short_type_name().to_string(),
            main_value: self.label.text().to_string(),
            ..Default::default()
        }
    }
}

fn update_label(config: &LapceConfig, label: &mut Label<LapceTabData>) {
    label.set_text_size(config.ui.font_size() as f64);
    label.set_font(FontDescriptor::new(config.ui.font_family()));
    label.set_text_color(
        config
            .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
            .clone(),
    );
}
