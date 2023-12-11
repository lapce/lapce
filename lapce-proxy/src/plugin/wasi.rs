#[cfg(test)]
mod tests;

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
use jsonrpc_lite::{Id, Params};
use lapce_core::directory::Directory;
use lapce_rpc::{
    plugin::{PluginId, VoltID, VoltInfo, VoltMetadata},
    style::LineStyle,
    RpcError,
};
use lapce_xi_rope::{Rope, RopeDelta};
use lsp_types::{
    notification::Initialized, request::Initialize, DocumentFilter,
    InitializeParams, InitializedParams, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, Url, VersionedTextDocumentIdentifier,
    WorkDoneProgressParams,
};
use parking_lot::Mutex;
use psp_types::{Notification, Request};
use wasi_experimental_http_wasmtime::{HttpCtx, HttpState};
use wasmtime_wasi::WasiCtxBuilder;

use super::{
    client_capabilities,
    psp::{
        handle_plugin_server_message, PluginHandlerNotification, PluginHostHandler,
        PluginServerHandler, PluginServerRpc, ResponseSender, RpcCallback,
    },
    volt_icon, PluginCatalogRpcHandler,
};
use crate::plugin::psp::PluginServerRpcHandler;

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
    configurations: Option<HashMap<String, serde_json::Value>>,
}

impl PluginServerHandler for Plugin {
    fn method_registered(&mut self, method: &str) -> bool {
        self.host.method_registered(method)
    }

    fn document_supported(
        &mut self,
        language_id: Option<&str>,
        path: Option<&Path>,
    ) -> bool {
        self.host.document_supported(language_id, path)
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
            SpawnedPluginLoaded { plugin_id } => {
                self.host.handle_spawned_plugin_loaded(plugin_id);
            }
        }
    }

    fn handle_host_notification(&mut self, method: String, params: Params) {
        let _ = self.host.handle_notification(method, params);
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
        let workspace = self.host.workspace.clone();
        let configurations = self.configurations.as_ref().map(unflatten_map);
        let root_uri = workspace.map(|p| Url::from_directory_path(p).unwrap());
        let server_rpc = self.host.server_rpc.clone();
        self.host.server_rpc.server_request_async(
            Initialize::METHOD,
            #[allow(deprecated)]
            InitializeParams {
                process_id: Some(process::id()),
                root_path: None,
                root_uri,
                capabilities: client_capabilities(),
                trace: None,
                client_info: None,
                locale: None,
                initialization_options: configurations,
                workspace_folders: None,
                work_done_progress_params: WorkDoneProgressParams::default(),
            },
            None,
            None,
            false,
            move |value| {
                if let Ok(value) = value {
                    if let Ok(result) = serde_json::from_value(value) {
                        server_rpc.handle_rpc(PluginServerRpc::Handler(
                            PluginHandlerNotification::InitializeResult(result),
                        ));
                        server_rpc.server_notification(
                            Initialized::METHOD,
                            InitializedParams {},
                            None,
                            None,
                            false,
                        );
                    }
                }
            },
        );
    }

    fn shutdown(&self) {}
}

pub fn load_all_volts(
    plugin_rpc: PluginCatalogRpcHandler,
    disabled_volts: Vec<VoltID>,
) {
    let all_volts = find_all_volts();
    let volts = all_volts
        .into_iter()
        .filter_map(|meta| {
            meta.wasm.as_ref()?;
            let icon = volt_icon(&meta);
            plugin_rpc.core_rpc.volt_installed(meta.clone(), icon);
            if disabled_volts.contains(&meta.id()) {
                return None;
            }
            Some(meta)
        })
        .collect();
    let _ = plugin_rpc.unactivated_volts(volts);
}

pub fn find_all_volts() -> Vec<VoltMetadata> {
    Directory::plugins_directory()
        .and_then(|d| {
            d.read_dir().ok().map(|dir| {
                dir.filter_map(|result| {
                    let entry = result.ok()?;
                    let metadata = entry.metadata().ok()?;

                    if metadata.is_file()
                        || entry.file_name().to_str()?.starts_with('.')
                    {
                        return None;
                    }
                    load_volt(&entry.path()).ok()
                })
                .collect()
            })
        })
        .unwrap_or_default()
}

/// Returns an instance of "VoltMetadata" or an error if there is no file in the path,
/// the contents of the file cannot be read into a string, or the content read cannot
/// be converted to an instance of "VoltMetadata".
///
/// # Examples
///
/// ```
/// use std::fs::File;
/// use std::io::Write;
/// use lapce_proxy::plugin::wasi::load_volt;
/// use lapce_rpc::plugin::VoltMetadata;
///
/// let parent_path = std::env::current_dir().unwrap();
/// let mut file = File::create(parent_path.join("volt.toml")).unwrap();
/// let _ = writeln!(file, "name = \"plugin\" \n version = \"0.1\"");
/// let _ = writeln!(file, "display-name = \"Plugin\" \n author = \"Author\"");
/// let _ = writeln!(file, "description = \"Useful plugin\"");///
/// let volt_metadata = match load_volt(&parent_path) {
///     Ok(volt_metadata) => volt_metadata,
///     Err(error) => panic!("{}", error),
/// };
/// assert_eq!(
///     volt_metadata,
///     VoltMetadata {
///         name: "plugin".to_string(),
///         version: "0.1".to_string(),
///         display_name: "Plugin".to_string(),
///         author: "Author".to_string(),
///         description: "Useful plugin".to_string(),
///         icon: None,
///         repository: None,
///         wasm: None,
///         color_themes: None,
///         icon_themes: None,
///         dir: parent_path.canonicalize().ok(),
///         activation: None,
///         config: None
///     }
/// );
/// let _ = std::fs::remove_file(parent_path.join("volt.toml"));
/// ```
pub fn load_volt(path: &Path) -> Result<VoltMetadata> {
    let path = path.canonicalize()?;
    let mut file = fs::File::open(path.join("volt.toml"))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut meta: VoltMetadata = toml::from_str(&contents)?;

    meta.dir = Some(path.clone());
    meta.wasm = meta.wasm.as_ref().and_then(|wasm| {
        Some(path.join(wasm).canonicalize().ok()?.to_str()?.to_string())
    });
    // FIXME: This does `meta.color_themes = Some([])` in case, for example,
    // it cannot find matching files, but in that case it should do `meta.color_themes = None`
    meta.color_themes = meta.color_themes.as_ref().map(|themes| {
        themes
            .iter()
            .filter_map(|theme| {
                Some(path.join(theme).canonicalize().ok()?.to_str()?.to_string())
            })
            .collect()
    });
    // FIXME: This does `meta.icon_themes = Some([])` in case, for example,
    // it cannot find matching files, but in that case it should do `meta.icon_themes = None`
    meta.icon_themes = meta.icon_themes.as_ref().map(|themes| {
        themes
            .iter()
            .filter_map(|theme| {
                Some(path.join(theme).canonicalize().ok()?.to_str()?.to_string())
            })
            .collect()
    });

    Ok(meta)
}

pub fn enable_volt(
    plugin_rpc: PluginCatalogRpcHandler,
    volt: VoltInfo,
) -> Result<()> {
    let path = Directory::plugins_directory()
        .ok_or_else(|| anyhow!("can't get plugin directory"))?
        .join(volt.id().to_string());
    let meta = load_volt(&path)?;
    plugin_rpc.unactivated_volts(vec![meta])?;
    Ok(())
}

pub fn start_volt(
    workspace: Option<PathBuf>,
    configurations: Option<HashMap<String, serde_json::Value>>,
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

    #[cfg(target_os = "linux")]
    let volt_libc = {
        match std::process::Command::new("ldd").arg("--version").output() {
            Ok(cmd) => {
                if String::from_utf8_lossy(&cmd.stdout)
                    .to_lowercase()
                    .split_terminator('\n')
                    .next()
                    .unwrap_or("")
                    .contains("musl")
                {
                    "musl"
                } else {
                    "glibc"
                }
            }
            _ => "glibc",
        }
    };

    #[cfg(not(target_os = "linux"))]
    let volt_libc = "";

    let stdin = Arc::new(RwLock::new(WasiPipe::new()));
    let stdout = Arc::new(RwLock::new(WasiPipe::new()));
    let stderr = Arc::new(RwLock::new(WasiPipe::new()));
    let wasi = WasiCtxBuilder::new()
        .inherit_env()?
        .env("VOLT_OS", std::env::consts::OS)?
        .env("VOLT_ARCH", std::env::consts::ARCH)?
        .env("VOLT_LIBC", volt_libc)?
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
            wasmtime_wasi::Dir::open_ambient_dir(
                volt_path,
                wasmtime_wasi::ambient_authority(),
            )?,
            "/",
        )?
        .build();
    let mut store = wasmtime::Store::new(&engine, wasi);

    let (io_tx, io_rx) = crossbeam_channel::unbounded();
    let rpc = PluginServerRpcHandler::new(meta.id(), None, None, io_tx);

    let local_rpc = rpc.clone();
    let local_stdin = stdin.clone();
    linker.func_wrap("lapce", "host_handle_rpc", move || {
        if let Ok(msg) = wasi_read_string(&stdout) {
            if let Some(resp) = handle_plugin_server_message(&local_rpc, &msg) {
                if let Ok(msg) = serde_json::to_string(&resp) {
                    let _ = writeln!(local_stdin.write().unwrap(), "{msg}");
                }
            }
        }
    })?;
    linker.func_wrap("lapce", "host_handle_stderr", move || {
        if let Ok(msg) = wasi_read_string(&stderr) {
            eprintln!("got stderr from plugin: {msg}");
        }
    })?;
    linker.module(&mut store, "", &module)?;
    thread::spawn(move || {
        let instance = linker.instantiate(&mut store, &module).unwrap();
        let handle_rpc = instance
            .get_func(&mut store, "handle_rpc")
            .ok_or_else(|| anyhow!("can't convet to function"))
            .unwrap()
            .typed::<(), ()>(&mut store)
            .unwrap();
        for msg in io_rx {
            if let Ok(msg) = serde_json::to_string(&msg) {
                let _ = writeln!(stdin.write().unwrap(), "{msg}");
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
            meta.display_name.clone(),
            meta.activation
                .iter()
                .flat_map(|m| m.language.iter().flatten())
                .cloned()
                .map(|s| DocumentFilter {
                    language: Some(s),
                    pattern: None,
                    scheme: None,
                })
                .chain(
                    meta.activation
                        .iter()
                        .flat_map(|m| m.workspace_contains.iter().flatten())
                        .cloned()
                        .map(|s| DocumentFilter {
                            language: None,
                            pattern: Some(s),
                            scheme: None,
                        }),
                )
                .collect(),
            plugin_rpc.core_rpc.clone(),
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

fn unflatten_map(map: &HashMap<String, serde_json::Value>) -> serde_json::Value {
    let mut new = serde_json::json!({});
    for (key, value) in map.iter() {
        let mut current = new.as_object_mut().unwrap();
        let parts: Vec<&str> = key.split('.').collect();
        let total_parts = parts.len();
        for (i, part) in parts.into_iter().enumerate() {
            if i + 1 < total_parts {
                if !current.get(part).map(|v| v.is_object()).unwrap_or(false) {
                    current.insert(part.to_string(), serde_json::json!({}));
                }
                current = current.get_mut(part).unwrap().as_object_mut().unwrap();
            } else {
                current.insert(part.to_string(), value.clone());
            }
        }
    }
    new
}
