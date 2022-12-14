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
use crossbeam_channel::Sender;
use jsonrpc_lite::{Id, JsonRpc, Params};
use lapce_rpc::RpcError;
use lsp_types::Url;
use parking_lot::Mutex;
use psp_types::Request;
use serde::Serialize;
use serde_json::Value;

use super::{dap_types, psp::ResponseHandler};

pub struct DapClient {
    io_tx: Sender<JsonRpc>,
    id_counter: Arc<AtomicU64>,
    server_pending: Arc<Mutex<HashMap<Id, ResponseHandler<Value, RpcError>>>>,
}

impl DapClient {
    pub fn new(
        workspace: Option<PathBuf>,
        server_uri: Url,
        args: Vec<String>,
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

        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout));
            loop {
                match crate::plugin::lsp::read_message(&mut reader) {
                    Ok(message_str) => {
                        println!("dap read message {message_str}");
                    }
                    Err(_err) => {
                        return;
                    }
                };
            }
        });

        Ok(Self {
            io_tx,
            id_counter: Arc::new(AtomicU64::new(0)),
            server_pending: Arc::new(Mutex::new(HashMap::new())),
        })
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

    pub(crate) fn initialize(&self) -> Result<()> {
        let params = dap_types::InitializeParams {
            client_id: Some("hx".to_owned()),
            client_name: Some("helix".to_owned()),
            adapter_id: "".to_string(),
            locale: Some("en-us".to_owned()),
            lines_start_at_one: Some(true),
            columns_start_at_one: Some(true),
            path_format: Some("path".to_owned()),
            supports_variable_type: Some(true),
            supports_variable_paging: Some(false),
            supports_run_in_terminal_request: Some(true),
            supports_memory_references: Some(false),
            supports_progress_reporting: Some(false),
            supports_invalidated_event: Some(false),
        };

        let resp = self.request(dap_types::Initialize::METHOD, params);
        println!("dap initilize result {resp:?}");

        Ok(())
    }

    fn request<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
    ) -> Result<Value, RpcError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.request_common(method, params, ResponseHandler::Chan(tx));
        rx.recv().unwrap_or_else(|_| {
            Err(RpcError {
                code: 0,
                message: "io error".to_string(),
            })
        })
    }

    fn request_common<P: Serialize>(
        &self,
        method: &'static str,
        params: P,
        rh: ResponseHandler<Value, RpcError>,
    ) {
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let id = Id::Num(id as i64);
        let params = Params::from(serde_json::to_value(params).unwrap());

        {
            let mut pending = self.server_pending.lock();
            pending.insert(id.clone(), rh);
        }
        let msg = JsonRpc::request_with_params(id, method, params);
        let _ = self.io_tx.send(msg);
    }
}
