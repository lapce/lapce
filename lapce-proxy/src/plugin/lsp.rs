#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::{
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{self, Child, Command, Stdio},
    sync::Arc,
    thread,
};

use anyhow::{anyhow, Result};
use jsonrpc_lite::{Id, Params};
use lapce_core::meta;
use lapce_rpc::{
    plugin::{PluginId, VoltID},
    style::LineStyle,
    RpcError,
};
use lapce_xi_rope::Rope;
use lsp_types::{
    notification::{Initialized, Notification},
    request::{Initialize, Request},
    *,
};
use parking_lot::Mutex;
use serde_json::Value;

use super::{
    client_capabilities,
    psp::{
        handle_plugin_server_message, PluginHandlerNotification, PluginHostHandler,
        PluginServerHandler, PluginServerRpcHandler, ResponseSender, RpcCallback,
    },
};
use crate::{buffer::Buffer, plugin::PluginCatalogRpcHandler};

const HEADER_CONTENT_LENGTH: &str = "content-length";
const HEADER_CONTENT_TYPE: &str = "content-type";

pub enum LspRpc {
    Request {
        id: u64,
        method: String,
        params: Params,
    },
    Notification {
        method: String,
        params: Params,
    },
    Response {
        id: u64,
        result: Value,
    },
    Error {
        id: u64,
        error: RpcError,
    },
}

pub struct LspClient {
    plugin_rpc: PluginCatalogRpcHandler,
    server_rpc: PluginServerRpcHandler,
    process: Child,
    workspace: Option<PathBuf>,
    host: PluginHostHandler,
    options: Option<Value>,
}

impl PluginServerHandler for LspClient {
    fn method_registered(&mut self, method: &str) -> bool {
        self.host.method_registered(method)
    }

    fn document_supported(
        &mut self,
        lanaguage_id: Option<&str>,
        path: Option<&Path>,
    ) -> bool {
        self.host.document_supported(lanaguage_id, path)
    }

    fn handle_handler_notification(
        &mut self,
        notification: PluginHandlerNotification,
    ) {
        use PluginHandlerNotification::*;
        match notification {
            Initialize => {
                self.initialize();
            }
            InitializeResult(result) => {
                self.host.server_capabilities = result.capabilities;
            }
            Shutdown => {
                self.shutdown();
            }
            SpawnedPluginLoaded { .. } => {}
        }
    }

    fn handle_host_request(
        &mut self,
        id: Id,
        method: String,
        params: Params,
        resp: ResponseSender,
    ) {
        self.host.handle_request(id, method, params, resp);
    }

    fn handle_host_notification(&mut self, method: String, params: Params) {
        let _ = self.host.handle_notification(method, params);
    }

    fn handle_did_save_text_document(
        &self,
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: lapce_xi_rope::Rope,
    ) {
        self.host.handle_did_save_text_document(
            language_id,
            path,
            text_document,
            text,
        );
    }

    fn handle_did_change_text_document(
        &mut self,
        language_id: String,
        document: lsp_types::VersionedTextDocumentIdentifier,
        delta: lapce_xi_rope::RopeDelta,
        text: lapce_xi_rope::Rope,
        new_text: lapce_xi_rope::Rope,
        change: Arc<
            Mutex<(
                Option<TextDocumentContentChangeEvent>,
                Option<TextDocumentContentChangeEvent>,
            )>,
        >,
    ) {
        self.host.handle_did_change_text_document(
            language_id,
            document,
            delta,
            text,
            new_text,
            change,
        );
    }

    fn format_semantic_tokens(
        &self,
        tokens: SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        self.host.format_semantic_tokens(tokens, text, f);
    }
}

impl LspClient {
    #[allow(clippy::too_many_arguments)]
    fn new(
        plugin_rpc: PluginCatalogRpcHandler,
        document_selector: DocumentSelector,
        workspace: Option<PathBuf>,
        volt_id: VoltID,
        volt_display_name: String,
        spawned_by: Option<PluginId>,
        plugin_id: Option<PluginId>,
        pwd: Option<PathBuf>,
        server_uri: Url,
        args: Vec<String>,
        options: Option<Value>,
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

        let mut writer = Box::new(BufWriter::new(stdin));
        let (io_tx, io_rx) = crossbeam_channel::unbounded();
        let server_rpc = PluginServerRpcHandler::new(
            volt_id.clone(),
            spawned_by,
            plugin_id,
            io_tx.clone(),
        );
        thread::spawn(move || {
            for msg in io_rx {
                if let Ok(msg) = serde_json::to_string(&msg) {
                    let msg =
                        format!("Content-Length: {}\r\n\r\n{}", msg.len(), msg);
                    let _ = writer.write(msg.as_bytes());
                    let _ = writer.flush();
                }
            }
        });

        let local_server_rpc = server_rpc.clone();
        let core_rpc = plugin_rpc.core_rpc.clone();
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stdout));
            loop {
                match read_message(&mut reader) {
                    Ok(message_str) => {
                        if let Some(resp) = handle_plugin_server_message(
                            &local_server_rpc,
                            &message_str,
                        ) {
                            let _ = io_tx.send(resp);
                        }
                    }
                    Err(_err) => {
                        core_rpc.log(
                            tracing::Level::ERROR,
                            format!("lsp server {server} stopped!"),
                        );
                        return;
                    }
                };
            }
        });

        let core_rpc = plugin_rpc.core_rpc.clone();
        thread::spawn(move || {
            let mut reader = Box::new(BufReader::new(stderr));
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(n) => {
                        if n == 0 {
                            return;
                        }
                        core_rpc.log(
                            tracing::Level::ERROR,
                            format!("lsp server stderr: {}", line.trim_end()),
                        );
                    }
                    Err(_) => {
                        return;
                    }
                }
            }
        });

        let host = PluginHostHandler::new(
            workspace.clone(),
            pwd,
            volt_id,
            volt_display_name,
            document_selector,
            plugin_rpc.core_rpc.clone(),
            server_rpc.clone(),
            plugin_rpc.clone(),
        );

        Ok(Self {
            plugin_rpc,
            server_rpc,
            process,
            workspace,
            host,
            options,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start(
        plugin_rpc: PluginCatalogRpcHandler,
        document_selector: DocumentSelector,
        workspace: Option<PathBuf>,
        volt_id: VoltID,
        volt_display_name: String,
        spawned_by: Option<PluginId>,
        plugin_id: Option<PluginId>,
        pwd: Option<PathBuf>,
        server_uri: Url,
        args: Vec<String>,
        options: Option<Value>,
    ) -> Result<PluginId> {
        let mut lsp = Self::new(
            plugin_rpc,
            document_selector,
            workspace,
            volt_id,
            volt_display_name,
            spawned_by,
            plugin_id,
            pwd,
            server_uri,
            args,
            options,
        )?;
        let plugin_id = lsp.server_rpc.plugin_id;

        let rpc = lsp.server_rpc.clone();
        thread::spawn(move || {
            rpc.mainloop(&mut lsp);
        });
        Ok(plugin_id)
    }

    fn initialize(&mut self) {
        let root_uri = self
            .workspace
            .clone()
            .map(|p| Url::from_directory_path(p).unwrap());
        #[allow(deprecated)]
        let params = InitializeParams {
            process_id: Some(process::id()),
            root_uri: root_uri.clone(),
            initialization_options: self.options.clone(),
            capabilities: client_capabilities(),
            trace: Some(TraceValue::Verbose),
            workspace_folders: root_uri.map(|uri| {
                vec![WorkspaceFolder {
                    name: uri.as_str().to_string(),
                    uri,
                }]
            }),
            client_info: Some(ClientInfo {
                name: meta::NAME.to_owned(),
                version: Some(meta::VERSION.to_owned()),
            }),
            locale: None,
            root_path: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        if let Ok(value) = self.server_rpc.server_request(
            Initialize::METHOD,
            params,
            None,
            None,
            false,
        ) {
            let result: InitializeResult = serde_json::from_value(value).unwrap();
            self.host.server_capabilities = result.capabilities;
            self.server_rpc.server_notification(
                Initialized::METHOD,
                InitializedParams {},
                None,
                None,
                false,
            );
            if self
                .plugin_rpc
                .plugin_server_loaded(self.server_rpc.clone())
                .is_err()
            {
                self.server_rpc.shutdown();
                self.shutdown();
            }
        }
        //     move |result| {
        //         if let Ok(value) = result {
        //             let result: InitializeResult =
        //                 serde_json::from_value(value).unwrap();
        //             server_rpc.handle_rpc(PluginServerRpc::Handler(
        //                 PluginHandlerNotification::InitializeDone(result),
        //             ));
        //         }
        //     },
        // );
    }

    fn shutdown(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
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
}

pub struct DocumentFilter {
    /// The document must have this language id, if it exists
    pub language_id: Option<String>,
    /// The document's path must match this glob, if it exists
    pub pattern: Option<globset::GlobMatcher>,
    // TODO: URI Scheme from lsp-types document filter
}
impl DocumentFilter {
    /// Constructs a document filter from the LSP version
    /// This ignores any fields that are badly constructed
    pub(crate) fn from_lsp_filter_loose(
        filter: &lsp_types::DocumentFilter,
    ) -> DocumentFilter {
        DocumentFilter {
            language_id: filter.language.clone(),
            // TODO: clean this up
            pattern: filter
                .pattern
                .as_deref()
                .map(globset::Glob::new)
                .and_then(Result::ok)
                .map(|x| globset::Glob::compile_matcher(&x)),
        }
    }
}

pub enum LspHeader {
    ContentType,
    ContentLength(usize),
}

fn parse_header(s: &str) -> Result<LspHeader> {
    let split: Vec<String> =
        s.splitn(2, ": ").map(|s| s.trim().to_lowercase()).collect();
    if split.len() != 2 {
        return Err(anyhow!("Malformed"));
    };
    match split[0].as_ref() {
        HEADER_CONTENT_TYPE => Ok(LspHeader::ContentType),
        HEADER_CONTENT_LENGTH => {
            Ok(LspHeader::ContentLength(split[1].parse::<usize>()?))
        }
        _ => Err(anyhow!("Unknown parse error occurred")),
    }
}

pub fn read_message<T: BufRead>(reader: &mut T) -> Result<String> {
    let mut buffer = String::new();
    let mut content_length: Option<usize> = None;

    loop {
        buffer.clear();
        let _ = reader.read_line(&mut buffer)?;
        // eprin
        match &buffer {
            s if s.trim().is_empty() => break,
            s => {
                match parse_header(s)? {
                    LspHeader::ContentLength(len) => content_length = Some(len),
                    LspHeader::ContentType => (),
                };
            }
        };
    }

    let content_length = content_length
        .ok_or_else(|| anyhow!("missing content-length header: {}", buffer))?;

    let mut body_buffer = vec![0; content_length];
    reader.read_exact(&mut body_buffer)?;

    let body = String::from_utf8(body_buffer)?;
    Ok(body)
}

pub fn get_change_for_sync_kind(
    sync_kind: TextDocumentSyncKind,
    buffer: &Buffer,
    content_change: &TextDocumentContentChangeEvent,
) -> Option<Vec<TextDocumentContentChangeEvent>> {
    match sync_kind {
        TextDocumentSyncKind::NONE => None,
        TextDocumentSyncKind::FULL => {
            let text_document_content_change_event =
                TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: buffer.get_document(),
                };
            Some(vec![text_document_content_change_event])
        }
        TextDocumentSyncKind::INCREMENTAL => Some(vec![content_change.clone()]),
        _ => None,
    }
}
