use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use druid::{
    AppDelegate, AppLauncher, Command, Env, Event, LocalizedString, Point, Region,
    Size, Target, Widget, WidgetExt, WidgetPod, WindowDesc, WindowHandle, WindowId,
    WindowState,
};
#[cfg(target_os = "macos")]
use druid::{Menu, MenuItem, SysMods};
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::Config,
    data::{
        LapceData, LapceTabLens, LapceWindowData, LapceWindowLens, LapceWorkspace,
        LapceWorkspaceType,
    },
    db::{TabsInfo, WindowInfo},
    proxy::VERSION,
};

use crate::{logging::override_log_levels, tab::LAPCE_TAB_META};
use crate::{tab::LapceTabHeader, window::LapceWindow};

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
const LOGO_PNG: &[u8] = include_bytes!("../../extra/images/logo.png");
#[cfg(target_os = "windows")]
const LOGO_ICO: &[u8] = include_bytes!("../../extra/windows/lapce.ico");

#[derive(Parser)]
#[clap(name = "Lapce")]
#[clap(version=*VERSION)]
struct Cli {
    #[clap(short, long, action)]
    new: bool,
    paths: Vec<PathBuf>,
}

pub fn build_window(data: &mut LapceWindowData) -> impl Widget<LapceData> {
    LapceWindow::new(data).lens(LapceWindowLens(data.window_id))
}

pub fn launch() {
    let cli = Cli::parse();
    let pwd = std::env::current_dir().unwrap_or_default();
    let paths: Vec<PathBuf> = cli.paths.iter().map(|p| pwd.join(p)).collect();
    if !cli.new && LapceData::check_local_socket(paths.clone()).is_ok() {
        return;
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
        .level(log::LevelFilter::Debug)
        .chain(override_log_levels(
            fern::Dispatch::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Warn
                } else {
                    log::LevelFilter::Off
                })
                .chain(std::io::stderr()),
        ));

    if let Some(log_file) = Config::log_file().and_then(|f| fern::log_file(f).ok()) {
        log_dispatch = log_dispatch.chain(
            fern::Dispatch::new()
                .level(log::LevelFilter::Debug)
                .level_for("lapce_data::keypress::key_down", log::LevelFilter::Off)
                .level_for("sled", log::LevelFilter::Off)
                .level_for("tracing", log::LevelFilter::Off)
                .level_for("druid::core", log::LevelFilter::Off)
                .level_for("druid::box_constraints", log::LevelFilter::Off)
                .level_for("cranelift_codegen", log::LevelFilter::Off)
                .level_for("wasmtime_cranelift", log::LevelFilter::Off)
                .level_for("regalloc", log::LevelFilter::Off)
                .level_for("hyper::proto", log::LevelFilter::Off)
                .chain(log_file),
        );
    }

    match log_dispatch.apply() {
        Ok(()) => (),
        Err(e) => eprintln!("Initialising logging failed {e:?}"),
    }

    let mut launcher = AppLauncher::new().delegate(LapceAppDelegate::new());
    let mut data = LapceData::load(launcher.get_external_handle(), paths);
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
    config: &Arc<Config>,
) -> WindowDesc<T>
where
    W: Widget<T> + 'static,
{
    // Check if the window would spawn in point (x: 0, y: 0)
    // (initial coordinates of top left corner on primary screen)
    // and isn't maximised, then calculate point to spawn center of
    // editor in center of primary screen
    let pos = if pos.x == 0.0 && pos.y == 0.0 && !maximised {
        let screens = druid::Screen::get_monitors();
        let mut screens = screens.iter().filter(|f| f.is_primary());
        // Get actual workspace rectangle excluding taskbars/menus
        match screens.next() {
            Some(screen) => {
                let screen_center_pos = screen.virtual_work_rect().center();
                // Position our window centered, not in center point
                Point::new(
                    screen_center_pos.x - size.width / 2.0,
                    screen_center_pos.y - size.height / 2.0,
                )
            }
            None => {
                log::error!("No primary display found. Are you running lapce in console-only/SSH/WSL?");
                pos
            }
        }
    } else {
        pos
    };
    let mut desc = WindowDesc::new_with_id(window_id, root)
        .title(LocalizedString::new("Lapce").with_placeholder("Lapce"))
        .with_min_size(Size::new(384.0, 384.0))
        .window_size(size)
        .set_position(pos);

    if cfg!(not(target_os = "macos")) {
        desc = desc.show_titlebar(!config.lapce.custom_titlebar);
    } else {
        desc = desc.show_titlebar(false);
    }

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

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
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
    use lapce_data::command::{
        CommandKind, LapceCommand, LapceWorkbenchCommand, LAPCE_COMMAND,
    };

    desc.menu(|_, _, _| {
        Menu::new("Lapce").entry(
            Menu::new("")
                .entry(MenuItem::new("About Lapce").command(Command::new(
                    LAPCE_COMMAND,
                    LapceCommand {
                        kind: CommandKind::Workbench(
                            LapceWorkbenchCommand::ShowAbout,
                        ),
                        data: None,
                    },
                    Target::Auto,
                )))
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
        ctx: &mut druid::DelegateCtx,
        _window_id: WindowId,
        event: druid::Event,
        data: &mut LapceData,
        _env: &Env,
    ) -> Option<Event> {
        match event {
            Event::ApplicationWillTerminate => {
                let _ = data.db.save_app(data);
                return None;
            }
            Event::ApplicationShouldHandleReopen(has_visible_windows) => {
                if !has_visible_windows {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::NewWindow(WindowId::next()),
                        Target::Global,
                    ));
                }
                return None;
            }
            Event::WindowGotFocus(window_id) => {
                data.active_window = Arc::new(window_id);
                return Some(event);
            }
            _ => {}
        };
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
        match cmd {
            cmd if cmd.is(LAPCE_TAB_META) => {
                let meta = cmd.get_unchecked(LAPCE_TAB_META).take().unwrap();

                let (size, pos) = data
                    .windows
                    .get(&meta.data.window_id)
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
                    data.latest_release.clone(),
                    data.panel_orders.clone(),
                    ctx.get_external_handle(),
                    &info,
                    data.db.clone(),
                );

                let mut tab = meta.data;
                tab.window_id = Arc::new(window_data.window_id);

                let tab_id = tab.id;
                window_data.tabs_order = Arc::new(vec![tab_id]);
                window_data.active_id = Arc::new(tab_id);
                window_data.tabs.clear();
                window_data.tabs.insert(tab_id, tab);

                let window_widget = LapceWindow {
                    mouse_pos: Point::ZERO,
                    tabs: vec![meta.widget],
                    tab_headers: [tab_id]
                        .iter()
                        .map(|tab_id| {
                            let tab_header =
                                LapceTabHeader::new().lens(LapceTabLens(*tab_id));
                            WidgetPod::new(tab_header)
                        })
                        .collect(),
                    dragable_area: Region::EMPTY,
                    tab_header_cmds: Vec::new(),
                    mouse_down_cmd: None,
                    #[cfg(not(target_os = "macos"))]
                    holding_click_rect: None,
                };
                let window_id = window_data.window_id;
                let root = window_widget.lens(LapceWindowLens(window_id));
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
            cmd if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::UpdateLatestRelease(release) => {
                        *Arc::make_mut(&mut data.latest_release) =
                            Some(release.clone());
                        return druid::Handled::Yes;
                    }
                    LapceUICommand::RestartToUpdate(process_path, release) => {
                        let _ = data.db.save_app(data);
                        let process_path = process_path.clone();
                        let release = release.clone();
                        std::thread::spawn(move || -> anyhow::Result<()> {
                            log::info!("start to down new versoin");
                            let src =
                                lapce_data::update::download_release(&release)?;
                            log::info!("start to extract");
                            let path =
                                lapce_data::update::extract(&src, &process_path)?;
                            log::info!("now restart {path:?}");
                            lapce_data::update::restart(&path)?;
                            Ok(())
                        });
                        return druid::Handled::Yes;
                    }
                    LapceUICommand::OpenPaths {
                        window_tab_id,
                        folders,
                        files,
                    } => {
                        if let Some((window_id, tab_id)) = window_tab_id {
                            if let Some(window_data) = data.windows.get(window_id) {
                                if let Some(tab_data) = window_data.tabs.get(tab_id)
                                {
                                    for folder in folders {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::ShowWindow,
                                            Target::Window(*window_id),
                                        ));
                                        let workspace = LapceWorkspace {
                                            kind: tab_data.workspace.kind.clone(),
                                            path: Some(folder.to_path_buf()),
                                            last_open: 0,
                                        };
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::NewTab(Some(workspace)),
                                            Target::Window(*window_id),
                                        ));
                                    }
                                    for file in files {
                                        ctx.submit_command(Command::new(
                                            LAPCE_UI_COMMAND,
                                            LapceUICommand::OpenFile(
                                                file.to_path_buf(),
                                                false,
                                            ),
                                            Target::Widget(*tab_id),
                                        ));
                                    }
                                    return druid::Handled::Yes;
                                }
                            }
                        }

                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::ShowWindow,
                            Target::Window(*data.active_window),
                        ));
                        for folder in folders {
                            let workspace = LapceWorkspace {
                                kind: LapceWorkspaceType::Local,
                                path: Some(folder.to_path_buf()),
                                last_open: 0,
                            };
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::NewTab(Some(workspace)),
                                Target::Window(*data.active_window),
                            ));
                        }
                        for file in files {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::OpenFile(file.to_path_buf(), false),
                                Target::Window(*data.active_window),
                            ));
                        }
                        return druid::Handled::Yes;
                    }
                    LapceUICommand::NewWindow(from_window_id) => {
                        let (size, pos) = data
                            .windows
                            .get(from_window_id)
                            // If maximised, use default dimensions instead
                            .filter(|win| !win.maximised)
                            .map(|win| (win.size, win.pos + (50.0, 50.0)))
                            .unwrap_or_else(|| {
                                data.db
                                    .get_last_window_info()
                                    .map(|i| (i.size, i.pos))
                                    .unwrap_or_else(|_| {
                                        (
                                            Size::new(800.0, 600.0),
                                            Point::new(0.0, 0.0),
                                        )
                                    })
                            });
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
                            data.latest_release.clone(),
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
                    LapceUICommand::CloseWindow(window_id) => {
                        ctx.submit_command(Command::new(
                            druid::commands::CLOSE_WINDOW,
                            (),
                            Target::Window(*window_id),
                        ));
                        let _ = data.db.save_app(data);
                        return druid::Handled::Yes;
                    }
                    _ => (),
                }
            }
            _ => (),
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
