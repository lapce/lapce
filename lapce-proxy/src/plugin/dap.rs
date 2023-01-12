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
use lapce_rpc::{core::CoreRpcHandler, RpcError};
use lsp_types::Url;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;

use crate::plugin::dap_types::Initialize;

use super::{
    dap_types,
    dap_types::{
        ConfigurationDone, DapEvent, DapPayload, DapRequest, DapResponse, Launch,
        Request, RunInTerminal, RunInTerminalArguments, RunInTerminalResponse,
    },
    psp::ResponseHandler,
};

pub struct DapClient {
    core_rpc: CoreRpcHandler,
    pub(crate) dap_rpc: DapRpcHandler,
}

impl DapClient {
    pub fn new(
        workspace: Option<PathBuf>,
        server_uri: Url,
        args: Vec<String>,
        core_rpc: CoreRpcHandler,
    ) -> Result<Self> {
        let server = match server_uri.scheme() {
            "file" => {
                let path = server_uri.to_file_path().map_err(|_| anyhow!(""))?;
                #[cfg(unix)]
                let _ = std::process::Command::new("chmod")
                    .arg("+x")
                    .arg(&path)
                    .output();
                path.to_str().ok_or_else(|| anyhow!(""))?.to_string()
            }
            "urn" => server_uri.path().to_string(),
            _ => return Err(anyhow!("uri not supported")),
        };

        let mut process = Self::process(workspace.as_ref(), &server, &args)?;
        let stdin = process.stdin.take().unwrap();
        let stdout = process.stdout.take().unwrap();
        let stderr = process.stderr.take().unwrap();

        let (io_tx, io_rx) = crossbeam_channel::unbounded();
        let mut writer = Box::new(BufWriter::new(stdin));
        let dap_rpc = DapRpcHandler::new(io_tx);
        thread::spawn(move || {
            for msg in io_rx {
                if let Ok(msg) = serde_json::to_string(&msg) {
                    let msg =
                        format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                    println!("writer write {msg}");
                    let _ = writer.write(msg.as_bytes());
                    let _ = writer.flush();
                }
            }
        });

        {
            let dap_rpc = dap_rpc.clone();
            let core_rpc = core_rpc.clone();
            thread::spawn(move || {
                let mut reader = Box::new(BufReader::new(stdout));
                loop {
                    match crate::plugin::lsp::read_message(&mut reader) {
                        Ok(message_str) => {
                            dap_rpc.handle_server_message(&message_str);
                        }
                        Err(_err) => {
                            core_rpc.log(
                                log::Level::Error,
                                format!("dap server {server} stopped!"),
                            );
                            return;
                        }
                    };
                }
            });
        }

        Ok(Self { core_rpc, dap_rpc })
    }

    fn process(
        workspace: Option<&PathBuf>,
        server: &str,
        args: &[String],
    ) -> Result<Child> {
        let mut process = Command::new(server);
        if let Some(workspace) = workspace {
            process.current_dir(workspace);
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

    fn handle_host_request(&self, req: &DapRequest) -> Result<Value> {
        match req.command.as_str() {
            RunInTerminal::COMMAND => {
                let value = req
                    .arguments
                    .as_ref()
                    .ok_or_else(|| anyhow!("no arguments"))?;
                let args: RunInTerminalArguments =
                    serde_json::from_value(value.clone())?;
                let command = args.args.join(" ");
                self.core_rpc.run_in_terminal(command);
                let resp = RunInTerminalResponse {
                    process_id: None,
                    shell_process_id: None,
                };
                let resp = serde_json::to_value(&resp)?;
                Ok(resp)
            }
            _ => Err(anyhow!("not implemented")),
        }
    }

    fn handle_host_event(&self, event: &DapEvent) -> Result<()> {
        match event {
            DapEvent::Initialized(_) => {
                let _ = self.dap_rpc.request::<ConfigurationDone>(());
            }
            DapEvent::Stopped(_) => todo!(),
            DapEvent::Continued(_) => todo!(),
            DapEvent::Exited(exited) => {
                println!("process exited {}", exited.exit_code);
            }
            DapEvent::Terminated(_) => todo!(),
            DapEvent::Thread(_) => todo!(),
            DapEvent::Output(_) => todo!(),
            DapEvent::Breakpoint { reason, breakpoint } => todo!(),
            DapEvent::Module { reason, module } => todo!(),
            DapEvent::LoadedSource { reason, source } => todo!(),
            DapEvent::Process(process) => {
                println!("process {process:?}");
            }
            DapEvent::Capabilities(_) => todo!(),
            DapEvent::Memory(_) => todo!(),
        }
        Ok(())
    }

    pub(crate) fn initialize(&self) -> Result<()> {
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
        // println!("dap initilize result {resp:?}");

        Ok(())
    }
}

pub enum DapRpc {
    HostRequest(DapRequest),
    HostEvent(DapEvent),
}

#[derive(Clone)]
pub struct DapRpcHandler {
    io_tx: Sender<DapPayload>,
    rpc_tx: Sender<DapRpc>,
    rpc_rx: Receiver<DapRpc>,
    seq_counter: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<u64, ResponseHandler<DapResponse, RpcError>>>>,
}

impl DapRpcHandler {
    fn new(io_tx: Sender<DapPayload>) -> Self {
        let (rpc_tx, rpc_rx) = crossbeam_channel::unbounded();
        Self {
            io_tx,
            rpc_rx,
            rpc_tx,
            seq_counter: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn mainloop(&self, dap_client: &mut DapClient) {
        let _ = dap_client.initialize();
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
                    println!("request {req:?} resp {resp:?}");
                    let _ = self.io_tx.send(DapPayload::Response(resp));
                }
                DapRpc::HostEvent(event) => {
                    let _ = dap_client.handle_host_event(&event);
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
        let response =
            serde_json::from_value(resp.body.ok_or_else(|| RpcError {
                code: 0,
                message: "no body in response".to_string(),
            })?)
            .map_err(|e| RpcError {
                code: 0,
                message: e.to_string(),
            })?;
        Ok(response)
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
        if let Ok(payload) = serde_json::from_str::<DapPayload>(message_str) {
            match payload {
                DapPayload::Request(req) => {
                    println!("got dap req {req:?}");
                    let _ = self.rpc_tx.send(DapRpc::HostRequest(req));
                }
                DapPayload::Event(event) => {
                    let _ = self.rpc_tx.send(DapRpc::HostEvent(event));
                }
                DapPayload::Response(resp) => {
                    self.handle_server_response(resp);
                    println!("got response");
                }
            }
        }
    }

    pub fn launch(&self, params: Value) -> Result<()> {
        let resp = self
            .request::<Launch>(params)
            .map_err(|e| anyhow!(e.message))?;
        println!("launch resp {resp:?}");
        Ok(())
    }
}
