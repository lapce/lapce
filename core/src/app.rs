use druid::{
    AppLauncher, Env, LocalizedString, Size, Widget, WidgetExt, WindowDesc
};

use crate::{
    data::{ LapceData, LapceWindowLens},
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
    let launcher = AppLauncher::new();
    let data = LapceData::load(launcher.get_external_handle());
    let root = build_window(&data);
    let window = WindowDesc::new(root)
        .title(LocalizedString::new("Lapce").with_placeholder("Lapce"))
        .show_titlebar(false)
        .window_size(Size::new(800.0, 600.0))
        .with_min_size(Size::new(800.0, 600.0));
    let launcher = launcher.with_window(window);
    let launcher = launcher.configure_env(|env, data| data.reload_env(env));
    launcher.launch(data).expect("launch failed");
}
