use xilem::{App, AppLauncher, View};

fn app_logic(count: &mut u32) -> impl View<u32> {
    xilem::button("button", |data| *data += 1)
}

pub fn launch() {
    let app = App::new(0, app_logic);
    AppLauncher::new(app).run();
}
