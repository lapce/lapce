#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    io::{BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    str::FromStr,
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::Sender;
use druid::{ExtEventSink, Target, WidgetId, WindowId};
use flate2::read::GzDecoder;
use lapce_core::{directory::Directory, meta};
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::{
    core::{CoreHandler, CoreNotification, CoreRequest, CoreRpcHandler},
    plugin::VoltID,
    proxy::{ProxyRpc, ProxyRpcHandler},
    stdio::stdio_transport,
    terminal::TermId,
    RequestId, RpcMessage,
};
use lapce_xi_rope::Rope;
use lsp_types::{LogMessageParams, MessageType, Url};
use parking_lot::Mutex;
use serde_json::Value;
use thiserror::Error;

use crate::{
    command::{LapceUICommand, LAPCE_UI_COMMAND},
    data::{LapceWorkspace, LapceWorkspaceType, SshHost},
    terminal::RawTerminal,
};

const UNIX_PROXY_SCRIPT: &[u8] = include_bytes!("../../extra/proxy.sh");
const WINDOWS_PROXY_SCRIPT: &[u8] = include_bytes!("../../extra/proxy.ps1");

pub enum TermEvent {
    NewTerminal(Arc<Mutex<RawTerminal>>),
    UpdateContent(Vec<u8>),
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
            LogMessage {
                message: LogMessageParams { message, typ },
            } => match typ {
                MessageType::ERROR => {
                    log::error!("{message}")
                }
                MessageType::WARNING => {
                    log::warn!("{message}")
                }
                MessageType::INFO => {
                    log::info!("{message}")
                }
                MessageType::LOG => {
                    log::debug!("{message}")
                }
                _ => {}
            },
            ShowMessage { title, message } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::NewMessage {
                        kind: message.typ,
                        title,
                        message: message.message,
                    },
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
            VoltInstalled { volt, icon } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltInstalled(volt, icon),
                    Target::Widget(self.tab_id),
                );
            }
            VoltInstalling { volt, error } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltInstalling(volt, error),
                    Target::Widget(self.tab_id),
                );
            }
            VoltRemoving { volt, error } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltRemoving(volt, error),
                    Target::Widget(self.tab_id),
                );
            }
            VoltRemoved {
                volt,
                only_installing,
            } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::VoltRemoved(volt, only_installing),
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
                    LapceUICommand::UpdateCompletion {
                        request_id,
                        input,
                        resp,
                        plugin_id,
                    },
                    Target::Widget(self.tab_id),
                );
            }
            SignatureHelpResponse {
                request_id,
                resp,
                plugin_id,
            } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateSignature {
                        request_id,
                        resp,
                        plugin_id,
                    },
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
        disabled_volts: Vec<VoltID>,
        plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
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
        disabled_volts: Vec<VoltID>,
        plugin_configurations: HashMap<String, HashMap<String, serde_json::Value>>,
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
            LapceWorkspaceType::RemoteSSH(ssh) => {
                self.start_remote(SshRemote { ssh })?;
            }
            #[cfg(windows)]
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
        let proxy_version = match *meta::RELEASE {
            "Debug" | "Nightly" => "nightly".to_string(),
            _ => format!("v{}", *meta::VERSION),
        };

        // start ssh CM connection in case where it doesn't handle
        // executing command properly on remote host
        // also print ssh debug output when used with LAPCE_DEBUG env
        match remote.command_builder().arg("lapce-no-command").output() {
            Ok(cmd) => {
                log::debug!(target: "lapce_data::proxy::start_remote::first_try", "{}", String::from_utf8_lossy(&cmd.stderr));
                log::debug!(target: "lapce_data::proxy::start_remote::first_try", "{}", String::from_utf8_lossy(&cmd.stdout));
            }
            Err(err) => {
                log::error!(target: "lapce_data::proxy::start_remote::first_try", "{err}");
                return Err(anyhow!(err));
            }
        }

        // Note about platforms:
        // Windows can use either cmd.exe, powershell.exe or pwsh.exe as
        // SSH shell, syntax logic varies significantly that's why we bet on
        // cmd.exe as it doesn't add unwanted newlines and use powershell only
        // for proxy install
        //
        // Unix-like systems due to POSIX, always have /bin/sh which should not
        // be necessary to use explicitly most of the time, as many wide-spread
        // shells retain similar syntax, although shells like Nushell might not
        // work (hopefully no one uses it as login shell)
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
                *meta::NAME
            ),
            Darwin => format!(
                "~/Library/Application\\ Support/dev.lapce.{}/proxy",
                *meta::NAME
            ),
            _ => {
                format!("~/.local/share/{}/proxy", (*meta::NAME).to_lowercase())
            }
        };

        let script_install = match platform {
            Windows => {
                let local_proxy_script =
                    Directory::proxy_directory().unwrap().join("proxy.ps1");

                let mut proxy_script = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&local_proxy_script)?;
                proxy_script.write_all(WINDOWS_PROXY_SCRIPT)?;

                let remote_proxy_script = "${env:TEMP}\\lapce-proxy.ps1";
                remote.upload_file(local_proxy_script, remote_proxy_script)?;

                let cmd = remote
                    .command_builder()
                    .args([
                        "powershell",
                        "-c",
                        remote_proxy_script,
                        "-version",
                        &proxy_version,
                        "-directory",
                        &remote_proxy_path,
                    ])
                    .output()?;
                log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stderr));
                log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stdout));

                cmd.status
            }
            _ => {
                let local_proxy_script =
                    Directory::proxy_directory().unwrap().join("proxy.sh");

                let mut proxy_script = std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&local_proxy_script)?;
                proxy_script.write_all(UNIX_PROXY_SCRIPT)?;

                let remote_proxy_script = "/tmp/lapce-proxy.sh";
                remote.upload_file(local_proxy_script, remote_proxy_script)?;

                let cmd = remote
                    .command_builder()
                    .args(["chmod", "+x", remote_proxy_script])
                    .output()?;
                log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stderr));
                log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stdout));

                let cmd = remote
                    .command_builder()
                    .args([remote_proxy_script, &proxy_version, &remote_proxy_path])
                    .output()?;
                log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stderr));
                log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stdout));

                cmd.status
            }
        };

        let remote_proxy_file = match platform {
            Windows => format!("{remote_proxy_path}\\lapce.exe"),
            _ => format!("{remote_proxy_path}/lapce"),
        };

        let proxy_filename = format!("lapce-proxy-{platform}-{architecture}");

        log::debug!(target: "lapce_data::proxy::start_remote", "remote proxy path: {remote_proxy_path}");

        if !script_install.success() {
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
                // remove possibly outdated proxy
                if local_proxy_file.exists() {
                    // TODO: add proper proxy version detection and update proxy
                    // when needed
                    std::fs::remove_file(&local_proxy_file)?;
                }
                let url = format!("https://github.com/lapce/lapce/releases/download/{proxy_version}/{proxy_filename}.gz");
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

                match platform {
                    // Windows creates all dirs in provided path
                    Windows => remote
                        .command_builder()
                        .arg("mkdir")
                        .arg(remote_proxy_path)
                        .status()?,
                    // Unix needs -p to do same
                    _ => remote
                        .command_builder()
                        .arg("mkdir")
                        .arg("-p")
                        .arg(remote_proxy_path)
                        .status()?,
                };

                remote.upload_file(&local_proxy_file, &remote_proxy_file)?;
                if platform != Windows {
                    remote
                        .command_builder()
                        .arg("chmod")
                        .arg("+x")
                        .arg(&remote_proxy_file)
                        .status()?;
                }
            }
        }

        let mut child = match platform {
            // Force cmd.exe usage to resolve %envvar% variables
            Windows => remote
                .command_builder()
                .args(["cmd", "/c"])
                .arg(&remote_proxy_file)
                .arg("--proxy")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?,
            _ => remote
                .command_builder()
                .arg(&remote_proxy_file)
                .arg("--proxy")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()?,
        };
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
        log::debug!(target: "lapce_data::proxy::start_remote", "process id: {}", child.id());

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
                log::debug!(target: "lapce_data::proxy::host_specification", "{}", &stdout);
                match stdout {
                    // If empty, then we probably deal with Windows and not Unix
                    // or something went wrong with command output
                    "" => {
                        // Try cmd explicitly
                        let cmd = remote
                            .command_builder()
                            .args([
                                "cmd",
                                "/c",
                                "echo %OS% %PROCESSOR_ARCHITECTURE%",
                            ])
                            .output();
                        match cmd {
                            Ok(cmd) => {
                                let stdout = String::from_utf8_lossy(&cmd.stdout)
                                    .to_lowercase();
                                let stdout = stdout.trim();
                                log::debug!(target: "lapce_data::proxy::host_specification", "{}", &stdout);
                                match stdout.split_once(' ') {
                                    Some((os, arch)) => {
                                        (parse_os(os), parse_arch(arch))
                                    }
                                    None => {
                                        // PowerShell fallback
                                        let cmd = remote
                                            .command_builder()
                                            .args(["echo", "\"${env:OS} ${env:PROCESSOR_ARCHITECTURE}\""])
                                            .output();
                                        match cmd {
                                            Ok(cmd) => {
                                                let stdout =
                                                    String::from_utf8_lossy(
                                                        &cmd.stdout,
                                                    )
                                                    .to_lowercase();
                                                let stdout = stdout.trim();
                                                log::debug!(target: "lapce_data::proxy::host_specification", "{}", &stdout);
                                                match stdout.split_once(' ') {
                                                    Some((os, arch)) => (
                                                        parse_os(os),
                                                        parse_arch(arch),
                                                    ),
                                                    None => (UnknownOS, UnknownArch),
                                                }
                                            }
                                            Err(e) => {
                                                log::error!(target: "lapce_data::proxy::host_specification", "{e}");
                                                (UnknownOS, UnknownArch)
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!(target: "lapce_data::proxy::host_specification", "{e}");
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
                log::error!(target: "lapce_data::proxy::host_specification", "{e}");
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
    ssh: SshHost,
}

impl SshRemote {
    #[cfg(windows)]
    const SSH_ARGS: &'static [&'static str] = &[];

    #[cfg(unix)]
    const SSH_ARGS: &'static [&'static str] = &[
        "-o",
        "ControlMaster=auto",
        "-o",
        "ControlPath=~/.ssh/cm_%C",
        "-o",
        "ControlPersist=30m",
        "-o",
        "ConnectTimeout=15",
    ];
}

impl Remote for SshRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut cmd = new_command("scp");

        cmd.args(Self::SSH_ARGS);

        if let Some(port) = self.ssh.port {
            cmd.arg("-P").arg(port.to_string());
        }

        let output = cmd
            .arg(local.as_ref())
            .arg(dbg!(format!("{}:{remote}", self.ssh.user_host())))
            .output()?;

        log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&output.stderr));
        log::debug!(target: "lapce_data::proxy::upload_file", "{}", String::from_utf8_lossy(&output.stdout));

        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("ssh");
        cmd.args(Self::SSH_ARGS);

        if let Some(port) = self.ssh.port {
            cmd.arg("-p").arg(port.to_string());
        }

        cmd.arg(self.ssh.user_host());

        if !std::env::var("LAPCE_DEBUG").unwrap_or_default().is_empty() {
            cmd.arg("-v");
        }

        cmd
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct WslDistro {
    pub name: String,
    pub default: bool,
}

#[cfg(windows)]
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

#[cfg(windows)]
struct WslRemote {
    distro: String,
}

#[cfg(windows)]
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
        if let Some((maybe_drive_letter, _)) = path.split_once(['/', '\\']) {
            let b = maybe_drive_letter.as_bytes();
            if b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from(path)
}

#[cfg(not(windows))]
pub fn path_from_url(url: &Url) -> PathBuf {
    url.to_file_path()
        .unwrap_or_else(|_| PathBuf::from(url.path()))
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
        v if v.ends_with("bsd") => Bsd,
        _ => UnknownOS,
    }
}
