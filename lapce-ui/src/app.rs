#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{path::PathBuf, process::Stdio, sync::Arc};

use clap::Parser;
use druid::{
    AppDelegate, AppLauncher, Command, Env, Event, LocalizedString, Point, Region,
    Size, Target, Widget, WidgetExt, WidgetPod, WindowDesc, WindowHandle, WindowId,
    WindowState,
};
#[cfg(target_os = "macos")]
use druid::{Menu, MenuItem, SysMods};
#[cfg(target_os = "macos")]
use lapce_core::command::{EditCommand, FocusCommand};
use lapce_core::meta;
use lapce_data::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    config::LapceConfig,
    data::{
        LapceData, LapceTabLens, LapceWindowData, LapceWindowLens, LapceWorkspace,
        LapceWorkspaceType,
    },
    db::{TabsInfo, WindowInfo},
};

use crate::{
    logging::override_log_levels,
    tab::{LapceTabHeader, LAPCE_TAB_META},
    window::LapceWindow,
};

#[derive(Parser)]
#[clap(name = "Lapce")]
#[clap(version=*meta::VERSION)]
#[derive(Debug)]
struct Cli {
    /// Launch new window even if Lapce is already running
    #[clap(short, long, action)]
    new: bool,
    /// Don't return instantly when opened in terminal
    #[clap(short, long, action)]
    wait: bool,
    paths: Vec<PathBuf>,
}

pub fn build_window(data: &mut LapceWindowData) -> impl Widget<LapceData> {
    LapceWindow::new(data).lens(LapceWindowLens(data.window_id))
}

pub fn launch() {
    // if PWD is not set, then we are not being launched via a terminal
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    if std::env::var("PWD").is_err() {
        load_shell_env();
    }

    let cli = Cli::parse();

    // small hack to unblock terminal if launched from it
    if !cli.wait {
        let mut args = std::env::args().collect::<Vec<_>>();
        args.push("--wait".to_string());
        let mut cmd = std::process::Command::new(&args[0]);
        #[cfg(target_os = "windows")]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        if let Err(why) = cmd
            .args(&args[1..])
            .stderr(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
        {
            eprintln!("Failed to launch lapce: {why}");
        };
        return;
    }
    let pwd = std::env::current_dir().unwrap_or_default();
    let paths: Vec<PathBuf> = cli
        .paths
        .iter()
        .map(|p| pwd.join(p).canonicalize().unwrap_or_default())
        .collect();
    if !cli.new && LapceData::try_open_in_existing_process(&paths).is_ok() {
        return;
    }

    #[cfg(feature = "updater")]
    lapce_data::update::cleanup();

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

    let log_file = LapceConfig::log_file();
    if let Some(log_file) = log_file.clone().and_then(|f| fern::log_file(f).ok()) {
        log_dispatch = log_dispatch.chain(
            fern::Dispatch::new()
                .level(log::LevelFilter::Debug)
                .level_for("lapce_data::keypress::key_down", log::LevelFilter::Off)
                .level_for("sled", log::LevelFilter::Off)
                .level_for("tracing", log::LevelFilter::Off)
                .level_for("druid::core", log::LevelFilter::Off)
                .level_for("druid::window", log::LevelFilter::Off)
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

    log_panics::Config::new()
        .backtrace_mode(log_panics::BacktraceMode::Resolved)
        .install_panic_hook();

    let mut launcher = AppLauncher::new().delegate(LapceAppDelegate::new());
    let mut data = LapceData::load(launcher.get_external_handle(), paths, log_file);

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
    config: &Arc<LapceConfig>,
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
        desc = desc.show_titlebar(!config.core.custom_titlebar);
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

#[cfg(not(target_os = "macos"))]
fn window_icon() -> Option<druid::Icon> {
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    const LOGO: &[u8] = include_bytes!("../../extra/images/logo.png");

    #[cfg(target_os = "windows")]
    const LOGO: &[u8] = include_bytes!("../../extra/windows/lapce.ico");

    let image = image::load_from_memory(LOGO)
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
        let settings = Menu::new("Settings...")
            .entry(
                MenuItem::new("Open Settings")
                    .command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::OpenSettings,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    ))
                    .hotkey(SysMods::Cmd, ","),
            )
            // MacOS doesn't like Cmd K Cmd S in its native spot for keyboard shortcuts
            // so do what VSCode does and put it in the title
            //
            // \u{2318} is the Unicode  the Command symbol on MacOS
            .entry(
                MenuItem::new("Open Keyboard Shortcuts [\u{2318}K \u{2318}S]")
                    .command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::OpenKeyboardShortcuts,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    )),
            );
        Menu::new("Lapce")
            .entry(
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
                    .entry(settings)
                    .separator()
                    .entry(
                        MenuItem::new("Hide Lapce")
                            .command(druid::commands::HIDE_APPLICATION)
                            .hotkey(SysMods::Cmd, "h"),
                    )
                    .entry(
                        MenuItem::new("Hide Others")
                            .command(druid::commands::HIDE_OTHERS)
                            .hotkey(SysMods::AltCmd, "h"),
                    )
                    .entry(
                        MenuItem::new("Show All").command(druid::commands::SHOW_ALL),
                    )
                    .separator()
                    .entry(
                        MenuItem::new("Quit Lapce")
                            .command(druid::commands::QUIT_APP)
                            .hotkey(SysMods::Cmd, "q"),
                    ),
            )
            .separator()
            .entry(
                Menu::new("File")
                    .entry(MenuItem::new("New File").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::NewFile,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .separator()
                    .entry(MenuItem::new("Open").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::OpenFile,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .entry(MenuItem::new("Open Folder").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::OpenFolder,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .separator()
                    .entry(MenuItem::new("Save").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::Save),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .entry(MenuItem::new("Save All").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::SaveAll,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .separator()
                    .entry(MenuItem::new("Close Folder").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::CloseFolder,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .entry(MenuItem::new("Close Window").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Workbench(
                                LapceWorkbenchCommand::CloseWindow,
                            ),
                            data: None,
                        },
                        Target::Auto,
                    ))),
            )
            .entry(
                Menu::new("Edit")
                    .entry(MenuItem::new("Cut").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Edit(EditCommand::ClipboardCut),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .entry(MenuItem::new("Copy").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Edit(EditCommand::ClipboardCopy),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .entry(MenuItem::new("Paste").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Edit(EditCommand::ClipboardPaste),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .separator()
                    .entry(MenuItem::new("Undo").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Edit(EditCommand::Undo),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .entry(MenuItem::new("Redo").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Edit(EditCommand::Redo),
                            data: None,
                        },
                        Target::Auto,
                    )))
                    .separator()
                    .entry(MenuItem::new("Find").command(Command::new(
                        LAPCE_COMMAND,
                        LapceCommand {
                            kind: CommandKind::Focus(FocusCommand::Search),
                            data: None,
                        },
                        Target::Auto,
                    ))),
            )
    })
}

/// Uses a login shell to load the correct shell environment for the current user.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn load_shell_env() {
    use std::process::Command;

    let shell = match std::env::var("SHELL") {
        Ok(s) => s,
        Err(_) => {
            // Shell variable is not set, so we can't determine the correct shell executable.
            // Silently failing, since logger is not set up yet.
            return;
        }
    };

    let mut command = Command::new(shell);

    command.args(["--login"]).args(["-c", "printenv"]);

    let env = match command.output() {
        Ok(output) => String::from_utf8(output.stdout).unwrap_or_default(),

        Err(_) => {
            // sliently ignoring since logger is not yet available
            return;
        }
    };

    env.split('\n')
        .filter_map(|line| line.split_once('='))
        .for_each(|(key, value)| {
            std::env::set_var(key, value);
        })
}

/// The delegate handler for Top-Level Druid events (terminate, new window, etc.)
struct LapceAppDelegate {}

impl LapceAppDelegate {
    pub fn new() -> Self {
        Self {}
    }

    fn new_window(
        window_id: &WindowId,
        ctx: &mut druid::DelegateCtx,
        data: &mut LapceData,
    ) {
        let (size, pos, current_panels) = data
            .windows
            .get(window_id)
            // If maximised, use default dimensions instead
            .filter(|win| !win.maximised)
            .map(|win| {
                (
                    win.size,
                    win.pos + (50.0, 50.0),
                    win.tabs.get(&win.active_id).map(|tab| (*tab.panel).clone()),
                )
            })
            .unwrap_or_else(|| {
                data.db
                    .get_last_window_info()
                    .map(|i| (i.size, i.pos, None))
                    .unwrap_or_else(|_| {
                        (Size::new(800.0, 600.0), Point::new(0.0, 0.0), None)
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
            data.update_in_process,
            data.log_file.clone(),
            current_panels,
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
                    // Create new window immediately
                    let new_window_id = WindowId::next();
                    Self::new_window(&new_window_id, ctx, data);
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
                    data.update_in_process,
                    data.log_file.clone(),
                    None,
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
                    draggable_area: Region::EMPTY,
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
                    LapceUICommand::UpdateStarted => {
                        data.update_in_process = true;
                    }
                    LapceUICommand::UpdateFailed => {
                        data.update_in_process = false;
                    }
                    LapceUICommand::RestartToUpdate(process_path, release) => {
                        let _ = data.db.save_app(data);
                        let process_path = process_path.clone();
                        let release = release.clone();
                        let event_sink = ctx.get_external_handle();
                        std::thread::spawn(move || {
                            let do_update = || -> anyhow::Result<()> {
                                log::info!("start to down new versoin");
                                let src =
                                    lapce_data::update::download_release(&release)?;

                                log::info!("start to extract");
                                let path = lapce_data::update::extract(
                                    &src,
                                    &process_path,
                                )?;

                                log::info!("now restart {path:?}");
                                lapce_data::update::restart(&path)?;

                                Ok(())
                            };

                            let _ = event_sink.submit_command(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::UpdateStarted,
                                Target::Global,
                            );
                            if let Err(err) = do_update() {
                                log::error!("Failed to update: {err}");
                                let _ = event_sink.submit_command(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::UpdateFailed,
                                    Target::Global,
                                );
                            }
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
                        Self::new_window(from_window_id, ctx, data);
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
