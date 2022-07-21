use std::sync::Arc;

use druid::{
    AppDelegate, AppLauncher, Command, Env, Event, LocalizedString, Point, Size,
    Widget, WidgetExt, WindowDesc, WindowHandle, WindowId, WindowState,
};
#[cfg(target_os = "macos")]
use druid::{Menu, MenuItem, SysMods};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::Config,
    data::{LapceData, LapceWindowData, LapceWindowLens},
    db::{TabsInfo, WindowInfo},
    proxy::VERSION,
};

use crate::logging::override_log_levels;
use crate::window::LapceWindow;

#[cfg(target_os = "linux")]
const LOGO_PNG: &[u8] = include_bytes!("../../extra/images/logo.png");
#[cfg(target_os = "windows")]
const LOGO_ICO: &[u8] = include_bytes!("../../extra/windows/lapce.ico");

pub fn build_window(data: &mut LapceWindowData) -> impl Widget<LapceData> {
    LapceWindow::new(data).lens(LapceWindowLens(data.window_id))
}

pub fn launch() {
    let mut args = std::env::args();
    let mut path = None;
    if args.len() > 1 {
        args.next();
        for arg in args {
            match arg.as_str() {
                "-v" | "--version" => {
                    println!("lapce v{VERSION}");
                    return;
                }
                "-h" | "--help" => {
                    println!("lapce [-h|--help] [-v|--version] [PATH]");
                    return;
                }
                v => {
                    if v.starts_with('-') {
                        eprintln!("lapce: unrecognized option: {v}");
                        std::process::exit(1)
                    } else {
                        path = Some(v.to_string())
                    }
                }
            }
        }
    }

    let mut log_dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(if cfg!(debug_assertions) {
            log::LevelFilter::Warn
        } else {
            log::LevelFilter::Off
        })
        .chain(std::io::stderr());

    if let Some(log_file) = Config::log_file().and_then(|f| fern::log_file(f).ok()) {
        log_dispatch = log_dispatch.chain(log_file);
    }

    log_dispatch = override_log_levels(log_dispatch);
    match log_dispatch.apply() {
        Ok(()) => (),
        Err(e) => eprintln!("Initialising logging failed {e:?}"),
    }

    let mut launcher = AppLauncher::new().delegate(LapceAppDelegate::new());
    let mut data = LapceData::load(launcher.get_external_handle(), path);
    for (_window_id, window_data) in data.windows.iter_mut() {
        let root = build_window(window_data);
        let window = new_window_desc(
            window_data.window_id,
            root,
            window_data.size,
            window_data.pos,
            window_data.maximised,
            &window_data.config,
        );
        launcher = launcher.with_window(window);
    }

    let launcher = launcher.configure_env(|env, data| data.reload_env(env));
    launcher.launch(data).expect("launch failed");
}

fn new_window_desc<W, T: druid::Data>(
    window_id: WindowId,
    root: W,
    size: Size,
    pos: Point,
    maximised: bool,
    _config: &Arc<Config>,
) -> WindowDesc<T>
where
    W: Widget<T> + 'static,
{
    let mut desc = WindowDesc::new_with_id(window_id, root)
        .show_titlebar(false)
        .title(LocalizedString::new("Lapce").with_placeholder("Lapce"))
        .with_min_size(Size::new(384.0, 384.0))
        .window_size(size)
        .set_position(pos);

    if maximised {
        desc = desc.set_window_state(WindowState::Maximized);
    }

    if let Some(icon) = window_icon() {
        desc = desc.with_window_icon(icon);
    }

    #[cfg(target_os = "macos")]
    {
        desc = macos_window_desc(desc);
    }

    desc
}

#[cfg(target_os = "macos")]
fn window_icon() -> Option<druid::Icon> {
    None
}

#[cfg(target_os = "linux")]
fn window_icon() -> Option<druid::Icon> {
    let image = image::load_from_memory(LOGO_PNG)
        .expect("Invalid Icon")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    Some(druid::Icon::from_rgba(rgba, width, height).expect("Failed to open icon"))
}

#[cfg(target_os = "windows")]
fn window_icon() -> Option<druid::Icon> {
    let image = image::load_from_memory(LOGO_ICO)
        .expect("Invalid Icon")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    Some(druid::Icon::from_rgba(rgba, width, height).expect("Failed to open icon"))
}

#[cfg(target_os = "macos")]
fn macos_window_desc<T: druid::Data>(desc: WindowDesc<T>) -> WindowDesc<T> {
    desc.menu(|_, _, _| {
        Menu::new("Lapce").entry(
            Menu::new("")
                .entry(MenuItem::new("About Lapce"))
                .separator()
                .entry(
                    MenuItem::new("Hide Lapce")
                        .command(druid::commands::HIDE_APPLICATION)
                        .hotkey(SysMods::Cmd, "h"),
                )
                .separator()
                .entry(
                    MenuItem::new("Quit Lapce")
                        .command(druid::commands::QUIT_APP)
                        .hotkey(SysMods::Cmd, "q"),
                ),
        )
    })
}

/// The delegate handler for Top-Level Druid events (terminate, new window, etc.)
struct LapceAppDelegate {}

impl LapceAppDelegate {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for LapceAppDelegate {
    fn default() -> Self {
        Self::new()
    }
}

impl AppDelegate<LapceData> for LapceAppDelegate {
    fn event(
        &mut self,
        _ctx: &mut druid::DelegateCtx,
        _window_id: WindowId,
        event: druid::Event,
        data: &mut LapceData,
        _env: &Env,
    ) -> Option<Event> {
        //FIXME: no event::aplicationWillTerminate is sent.
        if let Event::ApplicationWillTerminate = event {
            let _ = data.db.save_app(data);
            return None;
        }
        Some(event)
    }

    fn window_removed(
        &mut self,
        id: WindowId,
        data: &mut LapceData,
        _env: &Env,
        _ctx: &mut druid::DelegateCtx,
    ) {
        if let Some(window) = data.windows.remove(&id) {
            for (_, tab) in window.tabs.iter() {
                let _ = data.db.save_workspace(tab);
            }
            data.db.save_last_window(&window);
        }
    }

    fn command(
        &mut self,
        ctx: &mut druid::DelegateCtx,
        _target: druid::Target,
        cmd: &Command,
        data: &mut LapceData,
        _env: &Env,
    ) -> druid::Handled {
        if let Some(LapceUICommand::NewWindow(from_window_id)) =
            cmd.get(LAPCE_UI_COMMAND)
        {
            let (size, pos) = data
                .windows
                .get(from_window_id)
                // If maximised, use default dimensions instead
                .filter(|win| !win.maximised)
                .map(|win| (win.size, win.pos + (50.0, 50.0)))
                .unwrap_or((Size::new(800.0, 600.0), Point::new(0.0, 0.0)));
            let info = WindowInfo {
                size,
                pos,
                maximised: false,
                tabs: TabsInfo {
                    active_tab: 0,
                    workspaces: vec![],
                },
            };
            let mut window_data = LapceWindowData::new(
                data.keypress.clone(),
                data.panel_orders.clone(),
                ctx.get_external_handle(),
                &info,
                data.db.clone(),
            );
            let root = build_window(&mut window_data);
            let window_id = window_data.window_id;
            data.windows.insert(window_id, window_data.clone());
            let desc = new_window_desc(
                window_id,
                root,
                info.size,
                info.pos,
                info.maximised,
                &window_data.config,
            );
            ctx.new_window(desc);
            return druid::Handled::Yes;
        }
        druid::Handled::No
    }

    fn window_added(
        &mut self,
        _id: WindowId,
        _handle: WindowHandle,
        _data: &mut LapceData,
        _env: &Env,
        _ctx: &mut druid::DelegateCtx,
    ) {
    }
}
