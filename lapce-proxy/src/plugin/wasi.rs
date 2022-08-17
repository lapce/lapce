use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    process,
    sync::{Arc, RwLock},
    thread,
};

use anyhow::{anyhow, Result};
use jsonrpc_lite::Params;
use lapce_rpc::{
    plugin::{PluginId, VoltInfo, VoltMetadata},
    style::LineStyle,
    RpcError,
};
use lsp_types::{
    request::Initialize, ClientCapabilities, InitializeParams,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, Url,
    VersionedTextDocumentIdentifier,
};
use parking_lot::Mutex;
use psp_types::Request;
use toml_edit::easy as toml;
use wasi_experimental_http_wasmtime::{HttpCtx, HttpState};
use wasmtime_wasi::WasiCtxBuilder;
use xi_rope::{Rope, RopeDelta};

use crate::{directory::Directory, plugin::psp::PluginServerRpcHandler};

use super::{
    psp::{
        handle_plugin_server_message, PluginHandlerNotification, PluginHostHandler,
        PluginServerHandler, RpcCallback,
    },
    PluginCatalogRpcHandler,
};

#[derive(Default)]
pub struct WasiPipe {
    buffer: VecDeque<u8>,
}

impl WasiPipe {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Read for WasiPipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let amt = std::cmp::min(buf.len(), self.buffer.len());
        for (i, byte) in self.buffer.drain(..amt).enumerate() {
            buf[i] = byte;
        }
        Ok(amt)
    }
}

impl Write for WasiPipe {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buffer.extend(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for WasiPipe {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "can not seek in a pipe",
        ))
    }
}

pub struct Plugin {
    #[allow(dead_code)]
    id: PluginId,
    host: PluginHostHandler,
    configurations: Option<serde_json::Value>,
}

impl PluginServerHandler for Plugin {
    fn method_registered(&mut self, method: &'static str) -> bool {
        self.host.method_registered(method)
    }

    fn language_supported(&mut self, language_id: Option<&str>) -> bool {
        self.host.language_supported(language_id)
    }

    fn handle_handler_notification(
        &mut self,
        notification: PluginHandlerNotification,
    ) {
        use PluginHandlerNotification::*;
        match notification {
            Initilize => {
                self.initialize();
            }
            Shutdown => {
                self.shutdown();
            }
        }
    }

    fn handle_host_notification(&mut self, method: String, params: Params) {
        let _ = self.host.handle_notification(method, params);
    }

    fn handle_host_request(&mut self, id: u64, method: String, params: Params) {
        let _ = self.host.handle_request(id, method, params);
    }

    fn handle_did_save_text_document(
        &self,
        language_id: String,
        path: PathBuf,
        text_document: TextDocumentIdentifier,
        text: Rope,
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
        document: VersionedTextDocumentIdentifier,
        delta: RopeDelta,
        text: Rope,
        new_text: Rope,
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
        tokens: lsp_types::SemanticTokens,
        text: Rope,
        f: Box<dyn RpcCallback<Vec<LineStyle>, RpcError>>,
    ) {
        self.host.format_semantic_tokens(tokens, text, f);
    }
}

impl Plugin {
    fn initialize(&mut self) {
        let server_rpc = self.host.server_rpc.clone();
        let workspace = self.host.workspace.clone();
        let configurations = self.configurations.clone();
        thread::spawn(move || {
            let root_uri = workspace.map(|p| Url::from_directory_path(p).unwrap());
            let _ = server_rpc.server_request(
                Initialize::METHOD,
                #[allow(deprecated)]
                InitializeParams {
                    process_id: Some(process::id()),
                    root_path: None,
                    root_uri,
                    capabilities: ClientCapabilities::default(),
                    trace: None,
                    client_info: None,
                    locale: None,
                    initialization_options: configurations,
                    workspace_folders: None,
                },
                None,
                false,
            );
        });
    }

    fn shutdown(&self) {}
}

pub fn load_all_volts(
    workspace: Option<PathBuf>,
    plugin_rpc: PluginCatalogRpcHandler,
    disabled_volts: Vec<String>,
    volt_configurations: HashMap<String, serde_json::Value>,
) {
    let all_volts = find_all_volts();
    for meta in all_volts {
        if meta.wasm.is_none() {
            continue;
        }
        plugin_rpc.core_rpc.volt_installed(meta.clone());
        if disabled_volts.contains(&meta.id()) {
            continue;
        }
        let workspace = workspace.clone();
        let configurations = volt_configurations.get(&meta.name).cloned();
        let plugin_rpc = plugin_rpc.clone();
        thread::spawn(move || {
            if let Err(e) = start_volt(workspace, configurations, plugin_rpc, meta) {
                eprintln!("start volt error {e}");
            }
        });
    }
}

pub fn find_all_volts() -> Vec<VoltMetadata> {
    Directory::plugins_directory()
        .and_then(|d| {
            d.read_dir().ok().map(|dir| {
                dir.filter_map(|result| {
                    let entry = result.ok()?;
                    let path = entry.path().join("volt.toml");
                    load_volt(&path).ok()
                })
                .collect()
            })
        })
        .unwrap_or_default()
}

pub fn load_volt(path: &Path) -> Result<VoltMetadata> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut meta: VoltMetadata = toml::from_str(&contents)?;
    meta.dir = Some(path.parent().unwrap().canonicalize()?);
    meta.wasm = meta.wasm.as_ref().and_then(|wasm| {
        Some(
            path.parent()?
                .join(wasm)
                .canonicalize()
                .ok()?
                .to_str()?
                .to_string(),
        )
    });
    meta.themes = meta.themes.as_ref().map(|themes| {
        themes
            .iter()
            .filter_map(|theme| {
                Some(
                    path.parent()?
                        .join(theme)
                        .canonicalize()
                        .ok()?
                        .to_str()?
                        .to_string(),
                )
            })
            .collect()
    });
    Ok(meta)
}

pub fn start_volt_from_info(
    workspace: Option<PathBuf>,
    configurations: Option<serde_json::Value>,
    catalog_rpc: PluginCatalogRpcHandler,
    volt: VoltInfo,
) -> Result<()> {
    let path = Directory::plugins_directory()
        .ok_or_else(|| anyhow!("can't get plugin directory"))?
        .join(volt.id())
        .join("volt.toml");
    let meta = load_volt(&path)?;
    start_volt(workspace, configurations, catalog_rpc, meta)?;
    Ok(())
}

pub fn start_volt(
    workspace: Option<PathBuf>,
    configurations: Option<serde_json::Value>,
    plugin_rpc: PluginCatalogRpcHandler,
    meta: VoltMetadata,
) -> Result<()> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::from_file(
        &engine,
        meta.wasm
            .as_ref()
            .ok_or_else(|| anyhow!("no wasm in plugin"))?,
    )?;
    let mut linker = wasmtime::Linker::new(&engine);
    wasmtime_wasi::add_to_linker(&mut linker, |s| s)?;
    HttpState::new()?.add_to_linker(&mut linker, |_| HttpCtx {
        allowed_hosts: Some(vec!["insecure:allow-all".to_string()]),
        max_concurrent_requests: Some(100),
    })?;

    let volt_path = meta
        .dir
        .as_ref()
        .ok_or_else(|| anyhow!("plugin meta doesn't have dir"))?;

    let stdin = Arc::new(RwLock::new(WasiPipe::new()));
    let stdout = Arc::new(RwLock::new(WasiPipe::new()));
    let stderr = Arc::new(RwLock::new(WasiPipe::new()));
    let wasi = WasiCtxBuilder::new()
        .inherit_env()?
        .env("OS", std::env::consts::OS)?
        .env("ARCH", std::env::consts::ARCH)?
        .env(
            "VOLT_URI",
            Url::from_directory_path(volt_path)
                .map_err(|_| anyhow!("can't convert folder path to uri"))?
                .as_ref(),
        )?
        .stdin(Box::new(wasi_common::pipe::ReadPipe::from_shared(
            stdin.clone(),
        )))
        .stdout(Box::new(wasi_common::pipe::WritePipe::from_shared(
            stdout.clone(),
        )))
        .stderr(Box::new(wasi_common::pipe::WritePipe::from_shared(
            stderr.clone(),
        )))
        .preopened_dir(
            wasmtime_wasi::Dir::from_std_file(std::fs::File::open(volt_path)?),
            "/",
        )?
        .build();
    let mut store = wasmtime::Store::new(&engine, wasi);

    let (io_tx, io_rx) = crossbeam_channel::unbounded();
    let rpc = PluginServerRpcHandler::new(meta.name.clone(), io_tx);

    let local_rpc = rpc.clone();
    linker.func_wrap("lapce", "host_handle_rpc", move || {
        if let Ok(msg) = wasi_read_string(&stdout) {
            handle_plugin_server_message(&local_rpc, &msg);
        }
    })?;
    linker.func_wrap("lapce", "host_handle_stderr", move || {
        if let Ok(msg) = wasi_read_string(&stderr) {
            eprintln!("got stderr from plugin: {msg}");
        }
    })?;
    linker.module(&mut store, "", &module)?;
    let handle_rpc = linker
        .get(&mut store, "", "handle_rpc")
        .ok_or_else(|| anyhow!("no function in wasm"))?
        .into_func()
        .ok_or_else(|| anyhow!("can't convet to function"))?
        .typed::<(), (), _>(&mut store)?;

    thread::spawn(move || {
        for msg in io_rx {
            {
                let _ = writeln!(stdin.write().unwrap(), "{}\r", msg);
            }
            let _ = handle_rpc.call(&mut store, ());
        }
    });

    let id = PluginId::next();
    let mut plugin = Plugin {
        id,
        host: PluginHostHandler::new(
            workspace,
            meta.dir.clone(),
            meta.id(),
            None,
            rpc.clone(),
            plugin_rpc.clone(),
        ),
        configurations,
    };
    let local_rpc = rpc.clone();
    thread::spawn(move || {
        local_rpc.mainloop(&mut plugin);
    });

    if plugin_rpc.plugin_server_loaded(rpc.clone()).is_err() {
        rpc.shutdown();
    }
    Ok(())
}

fn wasi_read_string(stdout: &Arc<RwLock<WasiPipe>>) -> Result<String> {
    let mut buf = String::new();
    stdout.write().unwrap().read_to_string(&mut buf)?;
    Ok(buf)
}
