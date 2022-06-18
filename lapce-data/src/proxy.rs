use std::io::BufReader;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::{path::PathBuf, sync::Arc};

use anyhow::{anyhow, Result};
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;
use druid::Target;
use druid::{ExtEventSink, WidgetId};
use flate2::read::GzDecoder;
use lapce_proxy::dispatch::Dispatcher;
use lapce_rpc::buffer::BufferId;
use lapce_rpc::core::{CoreNotification, CoreRequest};
use lapce_rpc::plugin::PluginDescription;
use lapce_rpc::proxy::ProxyRequest;
use lapce_rpc::source_control::FileDiff;
use lapce_rpc::terminal::TermId;
use lapce_rpc::RpcHandler;
use lapce_rpc::{stdio_transport, Callback};
use lapce_rpc::{ControlFlow, Handler};
use lsp_types::CompletionItem;
use lsp_types::Position;
use lsp_types::Url;
use parking_lot::Mutex;
use serde_json::json;
use serde_json::Value;
use xi_rope::spans::SpansBuilder;
use xi_rope::{Interval, Rope, RopeDelta};

use crate::command::LapceUICommand;
use crate::command::LAPCE_UI_COMMAND;
use crate::config::Config;
use crate::data::{LapceWorkspace, LapceWorkspaceType};
use crate::terminal::RawTerminal;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

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

#[derive(Clone)]
pub struct LapceProxy {
    pub tab_id: WidgetId,
    rpc: RpcHandler,
    proxy_receiver: Arc<Receiver<Value>>,
    term_tx: Sender<(TermId, TermEvent)>,
    event_sink: ExtEventSink,
}

impl Handler for LapceProxy {
    type Notification = CoreNotification;
    type Request = CoreRequest;

    fn handle_notification(&mut self, rpc: Self::Notification) -> ControlFlow {
        use lapce_rpc::core::CoreNotification::*;
        match rpc {
            SemanticStyles {
                rev,
                buffer_id,
                path,
                styles,
                len,
            } => {
                let event_sink = self.event_sink.clone();
                let tab_id = self.tab_id;
                rayon::spawn(move || {
                    let mut styles_span = SpansBuilder::new(len);
                    for style in styles {
                        styles_span.add_span(
                            Interval::new(style.start, style.end),
                            style.style,
                        );
                    }
                    let styles_span = Arc::new(styles_span.build());
                    let _ = event_sink.submit_command(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateSemanticStyles(
                            buffer_id,
                            path,
                            rev,
                            styles_span,
                        ),
                        Target::Widget(tab_id),
                    );
                });
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
            InstalledPlugins { plugins } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::UpdateInstalledPlugins(plugins),
                    Target::Widget(self.tab_id),
                );
            }
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
            ProxyConnected {} => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::ProxyUpdateStatus(ProxyStatus::Connected),
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
            ListDir { .. } | DiffFiles { .. } => {}
            FileChange { event } => {
                let _ = self.event_sink.submit_command(
                    LAPCE_UI_COMMAND,
                    LapceUICommand::FileChange(event),
                    Target::Widget(self.tab_id),
                );
            }
        }
        ControlFlow::Continue
    }

    fn handle_request(&mut self, _rpc: Self::Request) -> Result<Value, Value> {
        Err(json!("unimplemented"))
    }
}

impl LapceProxy {
    pub fn new(
        tab_id: WidgetId,
        workspace: LapceWorkspace,
        term_tx: Sender<(TermId, TermEvent)>,
        event_sink: ExtEventSink,
    ) -> Self {
        let (proxy_sender, proxy_receiver) = crossbeam_channel::unbounded();
        let rpc = RpcHandler::new(proxy_sender);
        let proxy = Self {
            tab_id,
            rpc,
            proxy_receiver: Arc::new(proxy_receiver),
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
            let _ = local_proxy.start(workspace.clone());
            let _ = event_sink.submit_command(
                LAPCE_UI_COMMAND,
                LapceUICommand::ProxyUpdateStatus(ProxyStatus::Disconnected),
                Target::Widget(tab_id),
            );
        });

        proxy
    }

    fn start(&self, workspace: LapceWorkspace) -> Result<()> {
        if let Some(path) = workspace.path.as_ref() {
            self.initialize(path.clone());
        }
        let (core_sender, core_receiver) = crossbeam_channel::unbounded();
        match workspace.kind {
            LapceWorkspaceType::Local => {
                let proxy_receiver = (*self.proxy_receiver).clone();
                thread::spawn(move || {
                    let dispatcher = Dispatcher::new(core_sender);
                    let _ = dispatcher.mainloop(proxy_receiver);
                });
            }
            LapceWorkspaceType::RemoteSSH(user, host) => {
                self.start_remote(SshRemote { user, host }, core_sender)?;
            }
            LapceWorkspaceType::RemoteWSL => {
                let distro = WslDistro::all()?
                    .into_iter()
                    .find(|distro| distro.default)
                    .ok_or_else(|| anyhow!("no default distro found"))?
                    .name;
                self.start_remote(WslRemote { distro }, core_sender)?;
            }
        }

        let mut proxy = self.clone();
        let mut handler = self.clone();
        proxy.rpc.mainloop(core_receiver, &mut handler);

        Ok(())
    }

    fn start_remote(
        &self,
        remote: impl Remote,
        core_sender: Sender<Value>,
    ) -> Result<()> {
        let proxy_filename = format!("lapce-proxy-{VERSION}");
        let remote_proxy_file = format!("~/.lapce/{}", proxy_filename);

        let cmd = remote
            .command_builder()
            .arg("test")
            .arg("-e")
            .arg(&remote_proxy_file)
            .status()?;
        if !cmd.success() {
            let local_proxy_file = Config::dir()
                .ok_or_else(|| anyhow!("can't find config dir"))?
                .join(&proxy_filename);
            if !local_proxy_file.exists() {
                let url = format!("https://github.com/lapce/lapce/releases/download/v{VERSION}/lapce-proxy-linux.gz");
                let mut resp = reqwest::blocking::get(url).expect("request failed");
                let mut out = std::fs::File::create(&local_proxy_file)
                    .expect("failed to create file");
                let mut gz = GzDecoder::new(&mut resp);
                std::io::copy(&mut gz, &mut out).expect("failed to copy content");
            }

            remote
                .command_builder()
                .arg("mkdir")
                .arg("~/.lapce/")
                .status()?;

            remote.upload_file(&local_proxy_file, &remote_proxy_file)?;

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

        let proxy_receiver = (*self.proxy_receiver).clone();
        stdio_transport(stdin, proxy_receiver, stdout, core_sender);

        Ok(())
    }

    pub fn initialize(&self, workspace: PathBuf) {
        self.rpc.send_rpc_notification(
            "initialize",
            &json!({
                "workspace": workspace,
            }),
        )
    }

    pub fn terminal_close(&self, term_id: TermId) {
        self.rpc.send_rpc_notification(
            "terminal_close",
            &json!({
                "term_id": term_id,
            }),
        )
    }

    pub fn terminal_resize(&self, term_id: TermId, width: usize, height: usize) {
        self.rpc.send_rpc_notification(
            "terminal_resize",
            &json!({
                "term_id": term_id,
                "width": width,
                "height": height,
            }),
        )
    }

    pub fn terminal_write(&self, term_id: TermId, content: &str) {
        self.rpc.send_rpc_notification(
            "terminal_write",
            &json!({
                "term_id": term_id,
                "content": content,
            }),
        )
    }

    pub fn new_terminal(
        &self,
        term_id: TermId,
        cwd: Option<PathBuf>,
        shell: String,
        raw: Arc<Mutex<RawTerminal>>,
    ) {
        let _ = self.term_tx.send((term_id, TermEvent::NewTerminal(raw)));
        self.rpc.send_rpc_notification(
            "new_terminal",
            &json!({
                "term_id": term_id,
                "cwd": cwd,
                "shell": shell,
            }),
        )
    }

    pub fn git_commit(&self, message: &str, diffs: Vec<FileDiff>) {
        self.rpc.send_rpc_notification(
            "git_commit",
            &json!({
                "message": message,
                "diffs": diffs,
            }),
        )
    }

    pub fn git_checkout(&self, branch: &str) {
        self.rpc.send_rpc_notification(
            "git_checkout",
            &json!({
                "branch": branch,
            }),
        )
    }

    pub fn install_plugin(&self, plugin: &PluginDescription) {
        self.rpc
            .send_rpc_notification("install_plugin", &json!({ "plugin": plugin }));
    }

    pub fn get_buffer_head(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "buffer_head",
            &json!({ "buffer_id": buffer_id, "path": path, }),
            f,
        );
    }

    pub fn global_search(&self, pattern: String, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "global_search",
            &json!({ "pattern": pattern }),
            f,
        );
    }

    pub fn new_buffer(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "new_buffer",
            &json!({ "buffer_id": buffer_id, "path": path }),
            f,
        );
    }

    pub fn save_buffer_as(
        &self,
        buffer_id: BufferId,
        path: PathBuf,
        rev: u64,
        content: String,
        f: Box<dyn Callback>,
    ) {
        let request = ProxyRequest::SaveBufferAs {
            buffer_id,
            path,
            rev,
            content,
        };
        self.rpc.send_rpc_request_value_async(request, f);
    }

    pub fn update(&self, buffer_id: BufferId, delta: &RopeDelta, rev: u64) {
        self.rpc.send_rpc_notification(
            "update",
            &json!({
                "buffer_id": buffer_id,
                "delta": delta,
                "rev": rev,
            }),
        )
    }

    pub fn save(&self, rev: u64, buffer_id: BufferId, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "save",
            &json!({
                "rev": rev,
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn get_completion(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_completion",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn completion_resolve(
        &self,
        buffer_id: BufferId,
        completion_item: CompletionItem,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "completion_resolve",
            &json!({
                "buffer_id": buffer_id,
                "completion_item": completion_item,
            }),
            f,
        );
    }

    pub fn get_hover(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_hover",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_signature(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_signature",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_references(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_references",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_files(&self, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "get_files",
            &json!({
                "path": "path",
            }),
            f,
        );
    }

    pub fn read_dir(&self, path: &Path, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "read_dir",
            &json!({
                "path": path,
            }),
            f,
        );
    }

    pub fn get_definition(
        &self,
        request_id: usize,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_definition",
            &json!({
                "request_id": request_id,
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_document_symbols(&self, buffer_id: BufferId, f: Box<dyn Callback>) {
        self.rpc.send_rpc_request_async(
            "get_document_symbols",
            &json!({
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn get_code_actions(
        &self,
        buffer_id: BufferId,
        position: Position,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_code_actions",
            &json!({
                "buffer_id": buffer_id,
                "position": position,
            }),
            f,
        );
    }

    pub fn get_document_formatting(
        &self,
        buffer_id: BufferId,
        f: Box<dyn Callback>,
    ) {
        self.rpc.send_rpc_request_async(
            "get_document_formatting",
            &json!({
                "buffer_id": buffer_id,
            }),
            f,
        );
    }

    pub fn stop(&self) {
        self.rpc.send_rpc_notification("shutdown", &json!({}));
        // self.core_sender.send(json!({
        //     "method": "shutdown",
        //     "params": {},
        // }));
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
