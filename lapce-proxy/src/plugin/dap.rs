use std::{
    collections::HashMap,
    io::{BufReader, BufWriter, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    thread,
};

use anyhow::{anyhow, Result};
use crossbeam_channel::{Receiver, Sender};
use lapce_rpc::{
    dap_types::{
        self, ConfigurationDone, Continue, ContinueArguments, ContinueResponse,
        DapEvent, DapId, DapPayload, DapRequest, DapResponse, DapServer,
        DebuggerCapabilities, Disconnect, Initialize, Launch, Next, NextArguments,
        Pause, PauseArguments, Request, RunDebugConfig, RunInTerminal,
        RunInTerminalArguments, RunInTerminalResponse, Scope, Scopes,
        ScopesArguments, ScopesResponse, SetBreakpoints, SetBreakpointsArguments,
        SetBreakpointsResponse, Source, SourceBreakpoint, StackTrace,
        StackTraceArguments, StackTraceResponse, StepIn, StepInArguments, StepOut,
        StepOutArguments, Terminate, ThreadId, Threads, ThreadsResponse, Variable,
        Variables, VariablesArguments, VariablesResponse,
    },
    terminal::TermId,
    RpcError,
};
use parking_lot::Mutex;
use serde_json::Value;

use super::{
    psp::{ResponseHandler, RpcCallback},
    PluginCatalogRpcHandler,
};

pub struct DapClient {
    plugin_rpc: PluginCatalogRpcHandler,
    pub(crate) dap_rpc: DapRpcHandler,
    dap_server: DapServer,
    config: RunDebugConfig,
    breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>,
    term_id: Option<TermId>,
    capabilities: Option<DebuggerCapabilities>,
    terminated: bool,
    disconnected: bool,
    restarted: bool,
}

impl DapClient {
    pub fn new(
        dap_server: DapServer,
        config: RunDebugConfig,
        breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Result<Self> {
        let dap_rpc = DapRpcHandler::new(config.dap_id);

        Ok(Self {
            plugin_rpc,
            dap_server,
            config,
            dap_rpc,
            breakpoints,
            term_id: None,
            capabilities: None,
            terminated: false,
            disconnected: false,
            restarted: false,
        })
    }

    pub fn start(
        dap_server: DapServer,
        config: RunDebugConfig,
        breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Result<DapRpcHandler> {
        let mut dap = Self::new(dap_server, config, breakpoints, plugin_rpc)?;
        dap.start_process()?;

        let dap_rpc = dap.dap_rpc.clone();
        dap.initialize()?;

        {
            let dap_rpc = dap_rpc.clone();
            thread::spawn(move || {
                dap_rpc.mainloop(&mut dap);
            });
        }

        Ok(dap_rpc)
    }

    fn start_process(&self) -> Result<()> {
        let program = self.dap_server.program.clone();
        let mut process = Self::process(
            &program,
            &self.dap_server.args,
            self.dap_server.cwd.as_ref(),
        )?;
        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        // let stderr = process.stderr.take().unwrap();

        let dap_rpc = self.dap_rpc.clone();
        let io_rx = self.dap_rpc.io_rx.clone();
        let io_tx = self.dap_rpc.io_tx.clone();
        let mut writer = Box::new(BufWriter::new(stdin));
        thread::spawn(move || -> Result<()> {
            for msg in io_rx {
                if let Ok(msg) = serde_json::to_string(&msg) {
                    let msg =
                        format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                    writer.write_all(msg.as_bytes())?;
                    writer.flush()?;
                }
            }
            Ok(())
        });

        {
            let plugin_rpc = self.plugin_rpc.clone();
            thread::spawn(move || {
                let mut reader = Box::new(BufReader::new(stdout));
                loop {
                    match crate::plugin::lsp::read_message(&mut reader) {
                        Ok(message_str) => {
                            dap_rpc.handle_server_message(&message_str);
                        }
                        Err(_err) => {
                            let _ = io_tx.send(DapPayload::Event(
                                DapEvent::Initialized(None),
                            ));
                            plugin_rpc.core_rpc.log(
                                tracing::Level::ERROR,
                                format!("dap server {program} stopped!"),
                            );

                            dap_rpc.disconnected();
                            return;
                        }
                    };
                }
            });
        }

        Ok(())
    }

    fn process(
        server: &str,
        args: &[String],
        cwd: Option<&PathBuf>,
    ) -> Result<Child> {
        let mut process = Command::new(server);
        if let Some(cwd) = cwd {
            process.current_dir(cwd);
        }

        process.args(args);

        // CREATE_NO_WINDOW
        // (https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
        // TODO: We set this because
        #[cfg(target_os = "windows")]
        std::os::windows::process::CommandExt::creation_flags(
            &mut process,
            0x08000000,
        );
        let child = process
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        Ok(child)
    }

    fn handle_host_request(&mut self, req: &DapRequest) -> Result<Value> {
        match req.command.as_str() {
            RunInTerminal::COMMAND => {
                let value = req
                    .arguments
                    .as_ref()
                    .ok_or_else(|| anyhow!("no arguments"))?;
                let args: RunInTerminalArguments =
                    serde_json::from_value(value.clone())?;
                let mut config = self.config.clone();
                config.debug_command = Some(args.args);
                self.plugin_rpc.core_rpc.run_in_terminal(config);
                let (term_id, process_id) =
                    self.dap_rpc.termain_process_rx.recv()?;
                self.term_id = Some(term_id);
                let resp = RunInTerminalResponse {
                    process_id,
                    shell_process_id: None,
                };
                let resp = serde_json::to_value(resp)?;
                Ok(resp)
            }
            _ => Err(anyhow!("not implemented")),
        }
    }

    fn handle_host_event(&mut self, event: &DapEvent) -> Result<()> {
        match event {
            DapEvent::Initialized(_) => {
                for (path, breakpoints) in self.breakpoints.clone().into_iter() {
                    if let Ok(breakpoints) =
                        self.dap_rpc.set_breakpoints(path.clone(), breakpoints)
                    {
                        self.plugin_rpc.core_rpc.dap_breakpoints_resp(
                            self.config.dap_id,
                            path,
                            breakpoints.breakpoints.unwrap_or_default(),
                        );
                    }
                }
                // send dap configurations here
                let _ = self.dap_rpc.request::<ConfigurationDone>(());
            }
            DapEvent::Stopped(stopped) => {
                let all_threads_stopped =
                    stopped.all_threads_stopped.unwrap_or_default();
                let mut stack_frames = HashMap::new();
                if all_threads_stopped {
                    if let Ok(response) = self.dap_rpc.threads() {
                        for thread in response.threads {
                            if let Ok(frames) = self.dap_rpc.stack_trace(thread.id) {
                                stack_frames.insert(thread.id, frames.stack_frames);
                            }
                        }
                    }
                }

                let current_thread = if all_threads_stopped {
                    Some(stopped.thread_id.unwrap_or_default())
                } else {
                    stopped.thread_id
                };

                let active_frame = current_thread
                    .and_then(|thread_id| stack_frames.get(&thread_id))
                    .and_then(|stack_frames| stack_frames.first());

                let mut vars = Vec::new();
                if let Some(frame) = active_frame {
                    if let Ok(scopes) = self.dap_rpc.scopes(frame.id) {
                        for scope in scopes {
                            let result =
                                self.dap_rpc.variables(scope.variables_reference);
                            vars.push((scope, result.unwrap_or_default()));
                        }
                    }
                }

                self.plugin_rpc.core_rpc.dap_stopped(
                    self.config.dap_id,
                    stopped.clone(),
                    stack_frames,
                    vars,
                );

                // if all_threads_stopped {
                //     if let Ok(response) = self.dap_rpc.threads() {
                //         for thread in response.threads {
                //             self.fetch_stack_trace(thread.id);
                //         }
                //         self.select_thread_id(
                //             stopped.thread_id.unwrap_or_default(),
                //             false,
                //         );
                //     }
                // } else if let Some(thread_id) = stopped.thread_id {
                //     self.select_thread_id(thread_id, false);
                // }
            }
            DapEvent::Continued(_) => {
                self.plugin_rpc.core_rpc.dap_continued(self.dap_rpc.dap_id);
            }
            DapEvent::Exited(_exited) => {}
            DapEvent::Terminated(_) => {
                self.terminated = true;
                // self.plugin_rpc.core_rpc.dap_terminated(self.dap_rpc.dap_id);
                if let Some(term_id) = self.term_id {
                    self.plugin_rpc.proxy_rpc.terminal_close(term_id);
                }
                let _ = self.check_restart();
            }
            DapEvent::Thread { .. } => {}
            DapEvent::Output(_) => {}
            DapEvent::Breakpoint { .. } => {}
            DapEvent::Module { .. } => {}
            DapEvent::LoadedSource { .. } => {}
            DapEvent::Process(_) => {}
            DapEvent::Capabilities(_) => {}
            DapEvent::Memory(_) => {}
        }
        Ok(())
    }

    pub(crate) fn initialize(&mut self) -> Result<()> {
        let params = dap_types::InitializeParams {
            client_id: Some("lapce".to_owned()),
            client_name: Some("Lapce".to_owned()),
            adapter_id: "".to_string(),
            locale: Some("en-us".to_owned()),
            lines_start_at_one: Some(true),
            columns_start_at_one: Some(true),
            path_format: Some("path".to_owned()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(false),
            // See comment on dispatch of `NewTerminal`
            #[cfg(target_os = "windows")]
            supports_run_in_terminal_request: Some(false),
            #[cfg(not(target_os = "windows"))]
            supports_run_in_terminal_request: Some(true),
            supports_memory_references: Some(false),
            supports_progress_reporting: Some(false),
            supports_invalidated_event: Some(false),
        };

        let resp = self
            .dap_rpc
            .request::<Initialize>(params)
            .map_err(|e| anyhow!(e.message))?;
        self.capabilities = Some(resp);

        Ok(())
    }

    fn stop(&self) {
        let dap_rpc = self.dap_rpc.clone();
        if self
            .capabilities
            .as_ref()
            .and_then(|c| c.supports_terminate_request)
            .unwrap_or(false)
        {
            thread::spawn(move || {
                let _ = dap_rpc.terminate();
            });
        } else {
            thread::spawn(move || {
                let _ = dap_rpc.disconnect();
            });
        }
    }

    // check if the DAP was restared when we received terminated or disconnected
    // if the DAP doesn't suports terminate request, then we also need to wait for
    // disconnected
    fn check_restart(&mut self) -> Result<()> {
        if !self.restarted {
            return Ok(());
        }
        if !self
            .capabilities
            .as_ref()
            .and_then(|c| c.supports_terminate_request)
            .unwrap_or(false)
            && !self.disconnected
        {
            return Ok(());
        }

        self.restarted = false;

        if self.disconnected {
            self.start_process()?;
            self.initialize()?;
        }
        self.terminated = false;
        self.disconnected = false;

        let dap_rpc = self.dap_rpc.clone();
        let config = self.config.clone();
        thread::spawn(move || {
            let _ = dap_rpc.launch(&config);
        });

        Ok(())
    }

    fn restart(&mut self, breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>) {
        self.restarted = true;
        self.breakpoints = breakpoints;
        if !self.terminated {
            self.stop();
        } else {
            let _ = self.check_restart();
        }
    }
}

pub enum DapRpc {
    HostRequest(DapRequest),
    HostEvent(DapEvent),
    Stop,
    Restart(HashMap<PathBuf, Vec<SourceBreakpoint>>),
    Shutdown,
    Disconnected,
}

#[derive(Clone)]
pub struct DebuggerData {
    pub debugger_type: String,
    pub program: String,
    pub args: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct DapRpcHandler {
    pub dap_id: DapId,
    rpc_tx: Sender<DapRpc>,
    rpc_rx: Receiver<DapRpc>,
    io_tx: Sender<DapPayload>,
    io_rx: Receiver<DapPayload>,
    pub(crate) termain_process_tx: Sender<(TermId, Option<u32>)>,
    termain_process_rx: Receiver<(TermId, Option<u32>)>,
    seq_counter: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<u64, ResponseHandler<DapResponse, RpcError>>>>,
}

impl DapRpcHandler {
    fn new(dap_id: DapId) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();
        let (io_tx, io_rx) = crossbeam_channel::unbounded();
        let (termain_process_tx, termain_process_rx) =
            crossbeam_channel::unbounded();
        Self {
            dap_id,
            io_tx,
            io_rx,
            rpc_rx,
            rpc_tx,
            termain_process_tx,
            termain_process_rx,
            seq_counter: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mainloop(&self, dap_client: &mut DapClient) {
        for msg in &self.rpc_rx {
            match msg {
                DapRpc::HostRequest(req) => {
                    let result = dap_client.handle_host_request(&req);
                    let resp = DapResponse {
                        request_seq: req.seq,
                        success: result.is_ok(),
                        command: req.command.clone(),
                        message: result.as_ref().err().map(|e| e.to_string()),
                        body: result.ok(),
                    };
                    let _ = self.io_tx.send(DapPayload::Response(resp));
                }
                DapRpc::HostEvent(event) => {
                    let _ = dap_client.handle_host_event(&event);
                }
                DapRpc::Stop => {
                    dap_client.stop();
                }
                DapRpc::Restart(breakpoints) => {
                    dap_client.restart(breakpoints);
                }
                DapRpc::Shutdown => {
                    if let Some(term_id) = dap_client.term_id {
                        dap_client.plugin_rpc.proxy_rpc.terminal_close(term_id);
                    }
                    return;
                }
                DapRpc::Disconnected => {
                    dap_client.disconnected = true;
                    if let Some(term_id) = dap_client.term_id {
                        dap_client.plugin_rpc.proxy_rpc.terminal_close(term_id);
                    }
                    let _ = dap_client.check_restart();
                }
            }
        }
    }

    fn request_async<R: Request>(
        &self,
        params: R::Arguments,
        f: impl RpcCallback<R::Result, RpcError> + 'static,
    ) {
        self.request_common::<R>(
            R::COMMAND,
            params,
            ResponseHandler::Callback(Box::new(
                |result: Result<DapResponse, RpcError>| {
                    let result = match result {
                        Ok(resp) => {
                            if resp.success {
                                serde_json::from_value(resp.body.into()).map_err(
                                    |e| RpcError {
                                        code: 0,
                                        message: e.to_string(),
                                    },
                                )
                            } else {
                                Err(RpcError {
                                    code: 0,
                                    message: resp.message.unwrap_or_default(),
                                })
                            }
                        }
                        Err(e) => Err(e),
                    };
                    Box::new(f).call(result);
                },
            )),
        );
    }

    fn request<R: Request>(
        &self,
        params: R::Arguments,
    ) -> Result<R::Result, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common::<R>(R::COMMAND, params, ResponseHandler::Chan(tx));
        let resp = rx
            .recv_timeout(std::time::Duration::from_secs(30))
            .map_err(|_| RpcError {
                code: 0,
                message: "io error".to_string(),
            })??;
        if resp.success {
            let resp: R::Result =
                serde_json::from_value(resp.body.into()).map_err(|e| RpcError {
                    code: 0,
                    message: e.to_string(),
                })?;
            Ok(resp)
        } else {
            Err(RpcError {
                code: 0,
                message: resp.message.unwrap_or_default(),
            })
        }
    }

    fn request_common<R: Request>(
        &self,
        command: &'static str,
        arguments: R::Arguments,
        rh: ResponseHandler<DapResponse, RpcError>,
    ) {
        let seq = self.seq_counter.fetch_add(1, Ordering::Relaxed);
        let arguments: Value = serde_json::to_value(arguments).unwrap();

        {
            let mut pending = self.server_pending.lock();
            pending.insert(seq, rh);
        }
        let _ = self.io_tx.send(DapPayload::Request(DapRequest {
            seq,
            command: command.to_string(),
            arguments: Some(arguments),
        }));
    }

    fn handle_server_response(&self, resp: DapResponse) {
        if let Some(rh) = { self.server_pending.lock().remove(&resp.request_seq) } {
            rh.invoke(Ok(resp));
        }
    }

    pub fn handle_server_message(&self, message_str: &str) {
        if let Ok(payload) = serde_json::from_str::<DapPayload>(message_str) {
            match payload {
                DapPayload::Request(req) => {
                    let _ = self.rpc_tx.send(DapRpc::HostRequest(req));
                }
                DapPayload::Event(event) => {
                    let _ = self.rpc_tx.send(DapRpc::HostEvent(event));
                }
                DapPayload::Response(resp) => {
                    self.handle_server_response(resp);
                }
            }
        }
    }

    pub fn launch(&self, config: &RunDebugConfig) -> Result<()> {
        let params = serde_json::json!({
            "program": config.program,
            "args": config.args,
            "cwd": config.cwd,
            "runInTerminal": true,
        });
        let _resp = self
            .request::<Launch>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    pub fn stop(&self) {
        let _ = self.rpc_tx.send(DapRpc::Stop);
    }

    pub fn restart(&self, breakpoints: HashMap<PathBuf, Vec<SourceBreakpoint>>) {
        let _ = self.rpc_tx.send(DapRpc::Restart(breakpoints));
    }

    fn disconnected(&self) {
        let _ = self.rpc_tx.send(DapRpc::Disconnected);
    }

    pub fn disconnect(&self) -> Result<()> {
        self.request::<Disconnect>(())
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    fn terminate(&self) -> Result<()> {
        self.request::<Terminate>(())
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    pub fn set_breakpoints_async(
        &self,
        file: PathBuf,
        breakpoints: Vec<SourceBreakpoint>,
        f: impl RpcCallback<SetBreakpointsResponse, RpcError> + 'static,
    ) {
        let params = SetBreakpointsArguments {
            source: Source {
                path: Some(file),
                name: None,
                source_reference: None,
                presentation_hint: None,
                origin: None,
                sources: None,
                adapter_data: None,
                checksums: None,
            },
            breakpoints: Some(breakpoints),
            source_modified: Some(false),
        };
        self.request_async::<SetBreakpoints>(params, f);
    }

    pub fn set_breakpoints(
        &self,
        file: PathBuf,
        breakpoints: Vec<SourceBreakpoint>,
    ) -> Result<SetBreakpointsResponse> {
        let params = SetBreakpointsArguments {
            source: Source {
                path: Some(file),
                name: None,
                source_reference: None,
                presentation_hint: None,
                origin: None,
                sources: None,
                adapter_data: None,
                checksums: None,
            },
            breakpoints: Some(breakpoints),
            source_modified: Some(false),
        };
        let resp = self
            .request::<SetBreakpoints>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn continue_thread(&self, thread_id: ThreadId) -> Result<ContinueResponse> {
        let params = ContinueArguments { thread_id };
        let resp = self
            .request::<Continue>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn pause_thread(&self, thread_id: ThreadId) -> Result<()> {
        let params = PauseArguments { thread_id };
        self.request::<Pause>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    pub fn threads(&self) -> Result<ThreadsResponse> {
        let resp = self
            .request::<Threads>(())
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn stack_trace(&self, thread_id: ThreadId) -> Result<StackTraceResponse> {
        let params = StackTraceArguments {
            thread_id,
            ..Default::default()
        };
        let resp = self
            .request::<StackTrace>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
    }

    pub fn scopes(&self, frame_id: usize) -> Result<Vec<Scope>> {
        let args = ScopesArguments { frame_id };

        let response = self
            .request::<Scopes>(args)
            .map_err(|e| anyhow!(e.message))?;
        Ok(response.scopes)
    }

    pub fn scopes_async(
        &self,
        frame_id: usize,
        f: impl RpcCallback<ScopesResponse, RpcError> + 'static,
    ) {
        let args = ScopesArguments { frame_id };

        self.request_async::<Scopes>(args, f);
    }

    pub fn variables(&self, variables_reference: usize) -> Result<Vec<Variable>> {
        let args = VariablesArguments {
            variables_reference,
            filter: None,
            start: None,
            count: None,
            format: None,
        };

        let response = self
            .request::<Variables>(args)
            .map_err(|e| anyhow!(e.message))?;
        Ok(response.variables)
    }

    pub fn variables_async(
        &self,
        variables_reference: usize,
        f: impl RpcCallback<VariablesResponse, RpcError> + 'static,
    ) {
        let args = VariablesArguments {
            variables_reference,
            filter: None,
            start: None,
            count: None,
            format: None,
        };

        self.request_async::<Variables>(args, f);
    }

    pub fn next(&self, thread_id: ThreadId) {
        let args = NextArguments {
            thread_id,
            granularity: None,
        };

        self.request_async::<Next>(args, move |_| {});
    }

    pub fn step_in(&self, thread_id: ThreadId) {
        let args = StepInArguments {
            thread_id,
            target_id: None,
            granularity: None,
        };

        self.request_async::<StepIn>(args, move |_| {});
    }

    pub fn step_out(&self, thread_id: ThreadId) {
        let args = StepOutArguments {
            thread_id,
            granularity: None,
        };

        self.request_async::<StepOut>(args, move |_| {});
    }
}
