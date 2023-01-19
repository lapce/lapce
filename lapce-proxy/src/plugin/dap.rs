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
    core::CoreRpcHandler,
    dap_types::{
        self, ConfigurationDone, Continue, ContinueArguments, ContinueResponse,
        DapEvent, DapId, DapPayload, DapRequest, DapResponse, DebuggerCapabilities,
        Disconnect, Initialize, Launch, Request, RunDebugConfig, RunInTerminal,
        RunInTerminalArguments, RunInTerminalResponse, StackFrame, StackTrace,
        StackTraceArguments, StackTraceResponse, Terminate, ThreadId, Threads,
        ThreadsResponse,
    },
    terminal::TermId,
    RpcError,
};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use super::{psp::ResponseHandler, PluginCatalogRpcHandler};

pub struct DapClient {
    plugin_rpc: PluginCatalogRpcHandler,
    pub(crate) dap_rpc: DapRpcHandler,
    config: RunDebugConfig,
    thread_id: Option<ThreadId>,
    term_id: Option<TermId>,
    stack_frames: HashMap<ThreadId, Vec<StackFrame>>,
    active_frame: Option<usize>,
    capabilities: Option<DebuggerCapabilities>,
}

impl DapClient {
    pub fn new(
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        config: RunDebugConfig,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Result<Self> {
        let mut process = Self::process(&program, &args, cwd.as_ref())?;
        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let stderr = process.stderr.take().unwrap();

        let (io_tx, io_rx) = crossbeam_channel::unbounded();
        let mut writer = Box::new(BufWriter::new(stdin));
        let dap_rpc = DapRpcHandler::new(config.dap_id, io_tx);
        thread::spawn(move || {
            for msg in io_rx {
                if let Ok(msg) = serde_json::to_string(&msg) {
                    // println!("send to dap server: {msg}");
                    let msg =
                        format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                    let _ = writer.write(msg.as_bytes());
                    let _ = writer.flush();
                }
            }
        });

        {
            let dap_rpc = dap_rpc.clone();
            let plugin_rpc = plugin_rpc.clone();
            thread::spawn(move || {
                let mut reader = Box::new(BufReader::new(stdout));
                loop {
                    match crate::plugin::lsp::read_message(&mut reader) {
                        Ok(message_str) => {
                            dap_rpc.handle_server_message(&message_str);
                        }
                        Err(_err) => {
                            plugin_rpc.core_rpc.log(
                                log::Level::Error,
                                format!("dap server {program} stopped!"),
                            );
                            dap_rpc.shutdown();
                            let _ = plugin_rpc.dap_disconnected(dap_rpc.dap_id);
                            return;
                        }
                    };
                }
            });
        }

        Ok(Self {
            plugin_rpc,
            dap_rpc,
            config,
            thread_id: None,
            term_id: None,
            stack_frames: HashMap::new(),
            active_frame: None,
            capabilities: None,
        })
    }

    pub fn start(
        program: String,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        config: RunDebugConfig,
        plugin_rpc: PluginCatalogRpcHandler,
    ) -> Result<DapRpcHandler> {
        let mut dap = Self::new(program, args, cwd, config.clone(), plugin_rpc)?;
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

        #[cfg(target_os = "windows")]
        let process = process.creation_flags(0x08000000);
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
                let command = args.args.join(" ");
                let mut config = self.config.clone();
                config.debug_command = Some(command);
                self.plugin_rpc.core_rpc.run_in_terminal(config);
                let (term_id, process_id) =
                    self.dap_rpc.termain_process_rx.recv()?;
                self.term_id = Some(term_id);
                let resp = RunInTerminalResponse {
                    process_id: Some(process_id),
                    shell_process_id: None,
                };
                let resp = serde_json::to_value(&resp)?;
                Ok(resp)
            }
            _ => Err(anyhow!("not implemented")),
        }
    }

    fn handle_host_event(&mut self, event: &DapEvent) -> Result<()> {
        match event {
            DapEvent::Initialized(_) => {
                // send dap configurations here
                let result = self.dap_rpc.request::<ConfigurationDone>(());
            }
            DapEvent::Stopped(stopped) => {
                // println!("stopped {stopped:?}");
                self.plugin_rpc.core_rpc.dap_stopped(
                    self.config.dap_id,
                    stopped.thread_id.unwrap_or_default(),
                );
                let all_threads_stopped =
                    stopped.all_threads_stopped.unwrap_or_default();

                if all_threads_stopped {
                    if let Ok(response) = self.dap_rpc.threads() {
                        for thread in response.threads {
                            self.fetch_stack_trace(thread.id);
                        }
                        self.select_thread_id(
                            stopped.thread_id.unwrap_or_default(),
                            false,
                        );
                    }
                } else if let Some(thread_id) = stopped.thread_id {
                    self.select_thread_id(thread_id, false);
                }
            }
            DapEvent::Continued(_) => {
                self.plugin_rpc.core_rpc.dap_continued(self.dap_rpc.dap_id);
            }
            DapEvent::Exited(exited) => {}
            DapEvent::Terminated(_) => {
                self.plugin_rpc.core_rpc.dap_terminated(self.dap_rpc.dap_id);
            }
            DapEvent::Thread { .. } => {}
            DapEvent::Output(_) => todo!(),
            DapEvent::Breakpoint { reason, breakpoint } => todo!(),
            DapEvent::Module { reason, module } => todo!(),
            DapEvent::LoadedSource { reason, source } => todo!(),
            DapEvent::Process(process) => {}
            DapEvent::Capabilities(_) => todo!(),
            DapEvent::Memory(_) => todo!(),
        }
        Ok(())
    }

    pub(crate) fn initialize(&mut self) -> Result<()> {
        let params = dap_types::InitializeParams {
            client_id: Some("lapce".to_owned()),
            client_name: Some("Lapce".to_owned()),
            adapter_id: "".to_string(),
            locale: Some("en-us".to_owned()),
            lines_start_at_one: Some(false),
            columns_start_at_one: Some(false),
            path_format: Some("path".to_owned()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(false),
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

    fn select_thread_id(&mut self, thread_id: ThreadId, force: bool) {
        if !force && self.thread_id.is_some() {
            return;
        }

        self.thread_id = Some(thread_id);
        self.fetch_stack_trace(thread_id);

        let frame = self.stack_frames[&thread_id].get(0).cloned();
        if let Some(frame) = &frame {
            self.jump_to_stack_frame(frame);
        }
    }

    fn fetch_stack_trace(&mut self, thread_id: ThreadId) {
        let frames = match self.dap_rpc.stack_trace(thread_id) {
            Ok(frames) => frames.stack_frames,
            Err(_) => return,
        };
        self.stack_frames.insert(thread_id, frames);
        self.active_frame = Some(0);
    }

    fn jump_to_stack_frame(&self, frame: &StackFrame) {}

    fn stop(&self) {
        if self
            .capabilities
            .as_ref()
            .and_then(|c| c.supports_terminate_request)
            .unwrap_or(false)
        {
            println!("terminate");
            let _ = self.dap_rpc.terminate();
        } else {
            println!("discoonnect");
            let _ = self.dap_rpc.disconnect();
        }
    }
}

pub enum DapRpc {
    HostRequest(DapRequest),
    HostEvent(DapEvent),
    Stop,
    Shutdown,
}

#[derive(Clone)]
pub struct DapRpcHandler {
    pub dap_id: DapId,
    io_tx: Sender<DapPayload>,
    rpc_tx: Sender<DapRpc>,
    rpc_rx: Receiver<DapRpc>,
    pub(crate) termain_process_tx: Sender<(TermId, u32)>,
    termain_process_rx: Receiver<(TermId, u32)>,
    seq_counter: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<u64, ResponseHandler<DapResponse, RpcError>>>>,
}

impl DapRpcHandler {
    fn new(dap_id: DapId, io_tx: Sender<DapPayload>) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();
        let (termain_process_tx, termain_process_rx) =
            crossbeam_channel::unbounded();
        Self {
            dap_id,
            io_tx,
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
                DapRpc::Shutdown => {
                    if let Some(term_id) = dap_client.term_id {
                        dap_client.plugin_rpc.proxy_rpc.terminal_close(term_id);
                    }
                    return;
                }
            }
        }
    }

    fn request<R: Request>(
        &self,
        params: R::Arguments,
    ) -> Result<R::Result, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common(R::COMMAND, params, ResponseHandler::Chan(tx));
        let resp = rx.recv().map_err(|_| RpcError {
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

    fn request_common<P: Serialize>(
        &self,
        command: &'static str,
        arguments: P,
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
        // println!("received from dap server: {message_str}");
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

    pub fn launch(&self, params: Value) -> Result<()> {
        let resp = self
            .request::<Launch>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(())
    }

    pub fn stop(&self) {
        let _ = self.rpc_tx.send(DapRpc::Stop);
    }

    fn shutdown(&self) {
        let _ = self.rpc_tx.send(DapRpc::Shutdown);
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

    pub fn continue_thread(&self, thread_id: ThreadId) -> Result<ContinueResponse> {
        let params = ContinueArguments { thread_id };
        let resp = self
            .request::<Continue>(params)
            .map_err(|e| anyhow!(e.message))?;
        Ok(resp)
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
}
