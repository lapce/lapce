use druid::{
    AppLauncher, Env, LocalizedString, Size, Widget, WidgetExt, WindowDesc, WindowId,
};

use crate::{
    data::{watch_settings, LapceData, LapceWindowLens},
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
    let mut data = LapceData::load();
    let root = build_window(&data);
    let window = WindowDesc::new(root)
        .title(LocalizedString::new("Lapce").with_placeholder("Lapce"))
        .window_size(Size::new(800.0, 600.0))
        .with_min_size(Size::new(800.0, 600.0));
    let launcher = AppLauncher::with_window(window);
    let launcher = launcher.configure_env(|env, data| data.reload_env(env));
    for (_, win) in data.windows.iter_mut() {
        for (_, tab) in win.tabs.iter_mut() {
            tab.start_update_process(launcher.get_external_handle());
        }
    }
    watch_settings(launcher.get_external_handle());
    launcher
        .use_simple_logger()
        .launch(data)
        .expect("launch failed");
}
