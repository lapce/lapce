use druid::{
    AppLauncher, Env, LocalizedString, MenuDesc, Size, Widget, WidgetExt,
    WindowDesc, WindowId,
};

use crate::{
    data::{LapceData, LapceWindowLens},
    window::LapceWindowNew,
};

fn build_window(data: &LapceData) -> impl Widget<LapceData> {
    let (window_id, window_data) = data.windows.iter().next().unwrap();
    LapceWindowNew::new(window_data)
        .lens(LapceWindowLens(*window_id))
        .env_scope(|env: &mut Env, data: &LapceData| data.reload_env(env))
    // .debug_widget()
    // .debug_widget_id()
    // .debug_paint_layout()
    // .debug_invalidation()
}

pub fn lanuch() {
    let data = LapceData::load();
    let local_data = data.clone();
    let window = WindowDesc::new(move || build_window(&local_data))
        .title(LocalizedString::new("lapce").with_placeholder("Lapce"))
        .menu(MenuDesc::empty())
        .window_size(Size::new(800.0, 600.0))
        .with_min_size(Size::new(800.0, 600.0));
    let launcher = AppLauncher::with_window(window)
        .configure_env(|env, data| data.reload_env(env));
    launcher
        .use_simple_logger()
        .launch(data)
        .expect("launch failed");
}
