use std::collections::HashMap;
use std::io::BufReader;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::{path::PathBuf, str::FromStr, sync::Arc};

use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use druid::{ExtEventSink, WidgetId};
use druid::{Target, WindowId};
use flate2::read::GzDecoder;
use lapce_proxy::directory::Directory;
use lapce_proxy::dispatch::Dispatcher;
use lapce_proxy::APPLICATION_NAME;
pub use lapce_proxy::VERSION;
use lapce_rpc::core::{CoreHandler, CoreNotification, CoreRequest, CoreRpcHandler};
use lapce_rpc::proxy::{ProxyRpc, ProxyRpcHandler};
use lapce_rpc::stdio::stdio_transport;
use lapce_rpc::terminal::TermId;
use lapce_rpc::RequestId;
use lapce_rpc::RpcMessage;
use lsp_types::Url;
use parking_lot::Mutex;
use serde_json::Value;
use thiserror::Error;
use xi_rope::Rope;

use crate::command::LapceUICommand;
use crate::command::LAPCE_UI_COMMAND;
use crate::data::{LapceWorkspace, LapceWorkspaceType};
use crate::terminal::RawTerminal;

pub enum TermEvent {
    NewTerminal(Arc<Mutex<RawTerminal>>),
    UpdateContent(String),
    CloseTerminal,
}

#[derive(Clone, Copy, Debug)]
pub enum ProxyStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Error, Debug)]
pub enum RequestError {
    /// Error in deserializing to the expected value
    #[error("failed to deserialize")]
    Deser(serde_json::Error),
    #[error("response failed")]
    Rpc(Value),
}

#[derive(Clone, Copy, Error, Debug, PartialEq, Eq, strum_macros::Display)]
#[strum(ascii_case_insensitive)]
enum HostPlatform {
    UnknownOS,
    #[strum(serialize = "windows")]
    Windows,
    #[strum(serialize = "linux")]
    Linux,
    #[strum(serialize = "darwin")]
    Darwin,
    #[strum(serialize = "bsd")]
    Bsd,
}

/// serialise via strum to arch name that is used
/// in CI artefacts
#[derive(Clone, Copy, Error, Debug, PartialEq, Eq, strum_macros::Display)]
#[strum(ascii_case_insensitive)]
enum HostArchitecture {
    UnknownArch,
    #[strum(serialize = "x86_64")]
    AMD64,
    #[strum(serialize = "x86")]
    X86,
    #[strum(serialize = "aarch64")]
    ARM64,
    #[strum(serialize = "armv7")]
    ARM32v7,
    #[strum(serialize = "armhf")]
    ARM32v6,
}

#[derive(Clone)]
pub struct LapceProxy {
    pub tab_id: WidgetId,
    pub proxy_rpc: ProxyRpcHandler,
    pub core_rpc: CoreRpcHandler,
    term_tx: Sender<(TermId, TermEvent)>,
    event_sink: ExtEventSink,
}

impl CoreHandler for LapceProxy {
    fn handle_notification(&mut self, rpc: CoreNotification) {
        use CoreNotification::*;
        match rpc {
            OpenPaths {
                window_tab_id,
                folders,
                files,
            } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenPaths {
                        window_tab_id: window_tab_id.map(|(window_id, tab_id)| {
                            (
                                WindowId::from_usize(window_id),
                                WidgetId::from_usize(tab_id),
                            )
                        }),
                        folders,
                        files,
                    },
                    Target::Global,
                );
            }
            ProxyConnected {} => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ProxyUpdateStatus(ProxyStatus::Connected),
                    Target::Widget(self.tab_id),
                );
            }
            OpenFileChanged { path, content } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::OpenFileChanged {
                        path,
                        content: Rope::from(content),
                    },
                    Target::Widget(self.tab_id),
                );
            }
            ReloadBuffer { path, content, rev } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ReloadBuffer {
                        path,
                        rev,
                        content: Rope::from(content),
                    },
                    Target::Widget(self.tab_id),
                );
            }
            WorkspaceFileChange {} => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::WorkspaceFileChange,
                    Target::Widget(self.tab_id),
                );
            }
            PublishDiagnostics { diagnostics } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::PublishDiagnostics(diagnostics),
                    Target::Widget(self.tab_id),
                );
            }
            WorkDoneProgress { progress } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::WorkDoneProgress(progress),
                    Target::Widget(self.tab_id),
                );
            }
            HomeDir { path } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::HomeDir(path),
                    Target::Widget(self.tab_id),
                );
            }
            VoltInstalled { volt } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltInstalled(volt),
                    Target::Widget(self.tab_id),
                );
            }
            VoltRemoved { volt } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltRemoved(volt),
                    Target::Widget(self.tab_id),
                );
            }
            ListDir { .. } | DiffFiles { .. } => {}
            DiffInfo { diff } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateDiffInfo(diff),
                    Target::Widget(self.tab_id),
                );
            }
            UpdateTerminal { term_id, content } => {
                let _ = self
                    .term_tx
                    .send((term_id, TermEvent::UpdateContent(content)));
            }
            CloseTerminal { term_id } => {
                let _ = self.term_tx.send((term_id, TermEvent::CloseTerminal));
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::CloseTerminal(term_id),
                    Target::Widget(self.tab_id),
                );
            }
            CompletionResponse {
                request_id,
                input,
                resp,
                plugin_id,
            } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateCompletion(
                        request_id, input, resp, plugin_id,
                    ),
                    Target::Widget(self.tab_id),
                );
            }
            Log { level, message } => {
                if let Ok(level) = log::Level::from_str(&level) {
                    log::log!(level, "{}", message);
                }
            }
        }
    }

    fn handle_request(&mut self, _id: RequestId, _rpc: CoreRequest) {}
}

impl LapceProxy {
    pub fn new(
        window_id: WindowId,
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        disabled_volts: Vec<String>,
        plugin_configurations: HashMap<String, serde_json::Value>,
        term_tx: Sender<(TermId, TermEvent)>,
        event_sink: ExtEventSink,
    ) -> Self {
        let proxy_rpc = ProxyRpcHandler::new();
        let core_rpc = CoreRpcHandler::new();

        let proxy = Self {
            tab_id,
            proxy_rpc,
            core_rpc,
            term_tx,
            event_sink: event_sink.clone(),
        };

        let local_proxy = proxy.clone();
        thread::spawn(move || {
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::ProxyUpdateStatus(ProxyStatus::Connecting),
                Target::Widget(tab_id),
            );
            let _ = local_proxy.start(
                workspace.clone(),
                disabled_volts,
                plugin_configurations,
                window_id.to_usize(),
                tab_id.to_usize(),
            );
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::ProxyUpdateStatus(ProxyStatus::Disconnected),
                Target::Widget(tab_id),
            );
        });

        proxy
    }

    fn start(
        &self,
        workspace: LapceWorkspace,
        disabled_volts: Vec<String>,
        plugin_configurations: HashMap<String, serde_json::Value>,
        window_id: usize,
        tab_id: usize,
    ) -> Result<()> {
        self.proxy_rpc.initialize(
            workspace.path.clone(),
            disabled_volts,
            plugin_configurations,
            window_id,
            tab_id,
        );
        match workspace.kind {
            LapceWorkspaceType::Local => {
                let proxy_rpc = self.proxy_rpc.clone();
                let core_rpc = self.core_rpc.clone();

                thread::spawn(move || {
                    let mut dispatcher = Dispatcher::new(core_rpc, proxy_rpc);
                    let proxy_rpc = dispatcher.proxy_rpc.clone();
                    proxy_rpc.mainloop(&mut dispatcher);
                });
            }
            LapceWorkspaceType::RemoteSSH(user, host) => {
                self.start_remote(SshRemote { user, host })?;
            }
            LapceWorkspaceType::RemoteWSL => {
                let distro = WslDistro::all()?
                    .into_iter()
                    .find(|distro| distro.default)
                    .ok_or_else(|| anyhow!("no default distro found"))?
                    .name;
                self.start_remote(WslRemote { distro })?;
            }
        }

        let mut handler = self.clone();
        self.core_rpc.mainloop(&mut handler);

        Ok(())
    }

    fn start_remote(&self, remote: impl Remote) -> Result<()> {
        use HostPlatform::*;
        let (platform, architecture) = self.host_specification(&remote).unwrap();

        if platform == UnknownOS || architecture == HostArchitecture::UnknownArch {
            log::error!(target: "lapce_data::proxy::start_remote", "detected remote host: {platform}/{architecture}");
            return Err(anyhow!("Unknown OS and/or architecture"));
        }

        // ! Below paths have to be synced with what is
        // ! returned by Config::proxy_directory()
        let remote_proxy_path = match platform {
            Windows => format!(
                "%HOMEDRIVE%%HOMEPATH%\\AppData\\Local\\lapce\\{}\\data\\proxy",
                *APPLICATION_NAME
            ),
            Darwin => format!(
                "~/Library/Application Support/dev.lapce.{}/proxy",
                *APPLICATION_NAME
            ),
            _ => {
                format!("~/.local/share/{}/proxy", *APPLICATION_NAME).to_lowercase()
            }
        };

        let remote_proxy_file = match platform {
            Windows => format!("{remote_proxy_path}\\lapce.exe"),
            _ => format!("{remote_proxy_path}/lapce"),
        };

        let proxy_filename = format!("lapce-proxy-{}-{}", platform, architecture);

        log::debug!(target: "lapce_data::proxy::start_remote", "remote proxy path: {remote_proxy_path}");

        let cmd = match platform {
            Windows => remote
                .command_builder()
                .args(["dir", &remote_proxy_file])
                .status()?,
            _ => remote
                .command_builder()
                .arg("test")
                .arg("-e")
                .arg(&remote_proxy_file)
                .status()?,
        };
        if !cmd.success() {
            let local_proxy_file = Directory::proxy_directory()
                .ok_or_else(|| anyhow!("can't find proxy directory"))?
                .join(&proxy_filename);
            if !local_proxy_file.exists() {
                let url = format!("https://github.com/lapce/lapce/releases/download/{}/{proxy_filename}.gz", match *VERSION {
                    "debug" => "nightly".to_string(),
                    s if s.starts_with("nightly") => "nightly".to_string(),
                    _ => format!("v{}", *VERSION),
                });
                log::debug!(target: "lapce_data::proxy::start_remote", "proxy download URI: {url}");
                let mut resp = reqwest::blocking::get(url).expect("request failed");
                if resp.status().is_success() {
                    let mut out = std::fs::File::create(&local_proxy_file)
                        .expect("failed to create file");
                    let mut gz = GzDecoder::new(&mut resp);
                    std::io::copy(&mut gz, &mut out)
                        .expect("failed to copy content");
                } else {
                    log::error!(target: "lapce_data::proxy::start_remote", "proxy download failed with: {}", resp.status());
                }
            }

            if platform == Windows {
                remote
                    .command_builder()
                    .arg("mkdir")
                    .arg(remote_proxy_path)
                    .status()?;
            } else {
                remote
                    .command_builder()
                    .arg("mkdir")
                    .arg("-p")
                    .arg(remote_proxy_path)
                    .status()?;
            }

            remote.upload_file(&local_proxy_file, &remote_proxy_file)?;
        }

        if platform != Windows {
            remote
                .command_builder()
                .arg("chmod")
                .arg("+x")
                .arg(&remote_proxy_file)
                .status()?;
        }

        let mut child = remote
            .command_builder()
            .arg(&remote_proxy_file)
            .arg("--proxy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("can't find stdin"))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| anyhow!("can't find stdout"))?,
        );

        let (writer_tx, writer_rx) = crossbeam_channel::unbounded();
        let (reader_tx, reader_rx) = crossbeam_channel::unbounded();
        stdio_transport(stdin, writer_rx, stdout, reader_tx);

        let local_proxy_rpc = self.proxy_rpc.clone();
        let local_writer_tx = writer_tx.clone();
        thread::spawn(move || {
            for msg in local_proxy_rpc.rx() {
                match msg {
                    ProxyRpc::Request(id, rpc) => {
                        let _ = local_writer_tx.send(RpcMessage::Request(id, rpc));
                    }
                    ProxyRpc::Notification(rpc) => {
                        let _ = local_writer_tx.send(RpcMessage::Notification(rpc));
                    }
                    ProxyRpc::Shutdown => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return;
                    }
                }
            }
        });

        let core_rpc = self.core_rpc.clone();
        let proxy_rpc = self.proxy_rpc.clone();
        thread::spawn(move || {
            for msg in reader_rx {
                match msg {
                    RpcMessage::Request(id, req) => {
                        let writer_tx = writer_tx.clone();
                        let core_rpc = core_rpc.clone();
                        thread::spawn(move || match core_rpc.request(req) {
                            Ok(resp) => {
                                let _ =
                                    writer_tx.send(RpcMessage::Response(id, resp));
                            }
                            Err(e) => {
                                let _ = writer_tx.send(RpcMessage::Error(id, e));
                            }
                        });
                    }
                    RpcMessage::Notification(n) => {
                        core_rpc.notification(n);
                    }
                    RpcMessage::Response(id, resp) => {
                        proxy_rpc.handle_response(id, Ok(resp));
                    }
                    RpcMessage::Error(id, err) => {
                        proxy_rpc.handle_response(id, Err(err));
                    }
                }
            }
        });

        Ok(())
    }

    fn host_specification(
        &self,
        remote: &impl Remote,
    ) -> Result<(HostPlatform, HostArchitecture)> {
        use HostArchitecture::*;
        use HostPlatform::*;

        let cmd = remote.command_builder().args(["uname", "-sm"]).output();

        let spec = match cmd {
            Ok(cmd) => {
                let stdout = String::from_utf8_lossy(&cmd.stdout).to_lowercase();
                let stdout = stdout.trim();
                log::debug!(target: "lapce_data::proxy::host_platform", "{}", &stdout);
                match stdout {
                    "" => {
                        let cmd = remote
                            .command_builder()
                            .args(["echo", "%OS%", "%PROCESSOR_ARCHITECTURE%"])
                            .output();
                        match cmd {
                            Ok(cmd) => {
                                let stdout = String::from_utf8_lossy(&cmd.stdout)
                                    .to_lowercase();
                                let stdout = stdout.trim();
                                log::debug!(target: "lapce_data::proxy::host_platform", "{}", &stdout);
                                match stdout.split_once(' ') {
                                    Some((os, arch)) => {
                                        (parse_os(os), parse_arch(arch))
                                    }
                                    None => (UnknownOS, UnknownArch),
                                }
                            }
                            Err(e) => {
                                log::error!(target: "lapce_data::proxy::host_platform", "{e}");
                                (UnknownOS, UnknownArch)
                            }
                        }
                    }
                    v => {
                        if let Some((os, arch)) = v.split_once(' ') {
                            (parse_os(os), parse_arch(arch))
                        } else {
                            (UnknownOS, UnknownArch)
                        }
                    }
                }
            }
            Err(e) => {
                log::error!(target: "lapce_data::proxy::host_platform", "{e}");
                (UnknownOS, UnknownArch)
            }
        };
        Ok(spec)
    }

    pub fn new_terminal(
        &self,
        term_id: TermId,
        cwd: Option<PathBuf>,
        shell: String,
        raw: Arc<Mutex<RawTerminal>>,
    ) {
        let _ = self.term_tx.send((term_id, TermEvent::NewTerminal(raw)));
        self.proxy_rpc.new_terminal(term_id, cwd, shell);
    }

    pub fn stop(&self) {
        self.proxy_rpc.shutdown();
        self.core_rpc.shutdown();
    }
}

fn new_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd
}

trait Remote: Sized {
    fn home_dir(&self) -> Result<String> {
        let cmd = self
            .command_builder()
            .arg("echo")
            .arg("-n")
            .arg("$HOME")
            .stdout(Stdio::piped())
            .output()?;

        Ok(String::from_utf8(cmd.stdout)?)
    }

    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()>;

    fn command_builder(&self) -> Command;
}

struct SshRemote {
    user: String,
    host: String,
}

impl SshRemote {
    #[cfg(target_os = "windows")]
    const SSH_ARGS: &'static [&'static str] = &["-o", "ConnectTimeout=15"];

    #[cfg(not(target_os = "windows"))]
    const SSH_ARGS: &'static [&'static str] = &[
        "-o",
        "ControlMaster=auto",
        "-o",
        "ControlPath=~/.ssh/cm-%r@%h:%p",
        "-o",
        "ControlPersist=30m",
        "-o",
        "ConnectTimeout=15",
    ];

    fn command_builder(user: &str, host: &str) -> Command {
        let mut cmd = new_command("ssh");
        cmd.arg(format!("{}@{}", user, host)).args(Self::SSH_ARGS);
        cmd
    }
}

impl Remote for SshRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        new_command("scp")
            .args(Self::SSH_ARGS)
            .arg(local.as_ref())
            .arg(dbg!(format!("{}@{}:{remote}", self.user, self.host)))
            .status()?;
        Ok(())
    }

    fn command_builder(&self) -> Command {
        Self::command_builder(&self.user, &self.host)
    }
}

#[derive(Debug)]
struct WslDistro {
    pub name: String,
    pub default: bool,
}

impl WslDistro {
    fn all() -> Result<Vec<WslDistro>> {
        let cmd = new_command("wsl")
            .arg("-l")
            .arg("-v")
            .stdout(Stdio::piped())
            .output()?;

        if !cmd.status.success() {
            return Err(anyhow!("failed to execute `wsl -l -v`"));
        }

        let distros = String::from_utf16(bytemuck::cast_slice(&cmd.stdout))?
            .lines()
            .skip(1)
            .filter_map(|line| {
                let line = line.trim_start();
                let default = line.starts_with('*');
                let name = line
                    .trim_start_matches('*')
                    .trim_start()
                    .split(' ')
                    .next()?;
                Some(WslDistro {
                    name: name.to_string(),
                    default,
                })
            })
            .collect();

        Ok(distros)
    }
}

struct WslRemote {
    distro: String,
}

impl Remote for WslRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut wsl_path = Path::new(r"\\wsl.localhost\").join(&self.distro);
        if !wsl_path.exists() {
            wsl_path = Path::new(r#"\\wsl$"#).join(&self.distro);
        }
        wsl_path = if remote.starts_with('~') {
            let home_dir = self.home_dir()?;
            wsl_path.join(remote.replacen('~', &home_dir, 1))
        } else {
            wsl_path.join(remote)
        };
        std::fs::copy(local, wsl_path)?;
        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("wsl");
        cmd.arg("-d").arg(&self.distro).arg("--");
        cmd
    }
}

// Rust-analyzer returns paths in the form of "file:///<drive>:/...", which gets parsed into URL
// as "/<drive>://" which is then interpreted by PathBuf::new() as a UNIX-like path from root.
// This function strips the additional / from the beginning, if the first segment is a drive letter.
#[cfg(windows)]
pub fn path_from_url(url: &Url) -> PathBuf {
    let path = url.path();
    if let Some(path) = path.strip_prefix('/') {
        if let Some((maybe_drive_letter, _)) = path.split_once(&['/', '\\']) {
            let b = maybe_drive_letter.as_bytes();
            if b.len() == 2
                && matches!(
                    b[0],
                    b'a'..=b'z' | b'A'..=b'Z'
                )
                && b[1] == b':'
            {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from(path)
}

#[cfg(not(windows))]
pub fn path_from_url(url: &Url) -> PathBuf {
    PathBuf::from(url.path())
}

fn parse_arch(arch: &str) -> HostArchitecture {
    use HostArchitecture::*;
    // processor architectures be like that
    match arch {
        "amd64" | "x64" | "x86_64" => AMD64,
        "x86" | "i386" | "i586" | "i686" => X86,
        "arm" | "armhf" | "armv6" => ARM32v6,
        "armv7" | "armv7l" => ARM32v7,
        "arm64" | "armv8" | "aarch64" => ARM64,
        _ => UnknownArch,
    }
}

fn parse_os(os: &str) -> HostPlatform {
    use HostPlatform::*;
    match os {
        "linux" => Linux,
        "darwin" => Darwin,
        "windows_nt" => Windows,
        v => {
            if v.ends_with("bsd") {
                Bsd
            } else {
                UnknownOS
            }
        }
    }
}
