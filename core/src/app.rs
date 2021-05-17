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
    let root = build_window(&data);
    let window = WindowDesc::new(|| root)
        .title(LocalizedString::new("lapce").with_placeholder("Lapce"))
        .menu(MenuDesc::empty())
        .window_size(Size::new(800.0, 600.0))
        .with_min_size(Size::new(800.0, 600.0));
    let launcher = AppLauncher::with_window(window);
    let launcher = launcher.configure_env(|env, data| data.reload_env(env));
    launcher
        .use_simple_logger()
        .launch(data)
        .expect("launch failed");
}
