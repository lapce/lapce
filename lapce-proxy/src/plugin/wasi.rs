use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    thread,
};

use anyhow::{anyhow, Result};
use home::home_dir;
use lapce_rpc::plugin::{PluginDescription, PluginId};
use wasmer::{ChainableNamedResolver, ImportObject, Store, WasmerEnv};
use wasmer_wasi::{Pipe, WasiEnv, WasiState};

use crate::plugin::psp::PluginServerRpcHandler;

use super::{
    psp::{
        handle_plugin_server_message, PluginHandlerNotification, PluginServerHandler,
    },
    PluginRpcHandler,
};

#[derive(WasmerEnv, Clone)]
pub struct NewPluginEnv {
    id: PluginId,
    plugin_rpc: PluginRpcHandler,
    rpc: PluginServerRpcHandler,
    wasi_env: WasiEnv,
    desc: PluginDescription,
}

#[derive(Clone)]
pub struct NewPlugin {
    id: PluginId,
    instance: wasmer::Instance,
    env: NewPluginEnv,
}

impl PluginServerHandler for NewPlugin {
    fn method_registered(&mut self, method: &'static str) -> bool {
        todo!()
    }

    fn handle_host_notification(&mut self) {
        todo!()
    }

    fn handle_handler_notification(
        &mut self,
        notification: PluginHandlerNotification,
    ) {
        todo!()
    }
}

pub fn load_all_plugins(plugin_rpc: PluginRpcHandler) {
    eprintln!("start to load plugins");
    let all_plugins = find_all_plugins();
    for plugin_path in &all_plugins {
        match load_plugin(plugin_path) {
            Err(_e) => (),
            Ok(plugin_desc) => {
                if let Err(e) = start_plugin(plugin_rpc.clone(), plugin_desc) {
                    eprintln!("start plugin error {}", e);
                }
            }
        }
    }
}

fn load_plugin(path: &Path) -> Result<PluginDescription> {
    let mut file = fs::File::open(&path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let mut plugin: PluginDescription = toml::from_str(&contents)?;
    plugin.dir = Some(path.parent().unwrap().canonicalize()?);
    plugin.wasm = plugin.wasm.as_ref().and_then(|wasm| {
        Some(
            path.parent()?
                .join(wasm)
                .canonicalize()
                .ok()?
                .to_str()?
                .to_string(),
        )
    });
    plugin.themes = plugin.themes.as_ref().map(|themes| {
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
    Ok(plugin)
}

fn start_plugin(
    plugin_rpc: PluginRpcHandler,
    plugin_desc: PluginDescription,
) -> Result<()> {
    eprintln!("start a certain plugin");
    let store = Store::default();
    let module = wasmer::Module::from_file(
        &store,
        plugin_desc
            .wasm
            .as_ref()
            .ok_or_else(|| anyhow!("no wasm in plugin"))?,
    )?;

    let output = Pipe::new();
    let input = Pipe::new();
    let env = plugin_desc.get_plugin_env()?;
    let mut wasi_env = WasiState::new("Lapce")
        .map_dir("/", plugin_desc.dir.clone().unwrap())?
        .stdin(Box::new(input))
        .stdout(Box::new(output))
        .envs(env)
        .finalize()?;
    let wasi = wasi_env.import_object(&module)?;

    let (io_tx, io_rx) = crossbeam_channel::unbounded();
    let rpc = PluginServerRpcHandler::new(io_tx);

    let id = PluginId::next();
    let plugin_env = NewPluginEnv {
        id,
        rpc: rpc.clone(),
        wasi_env,
        plugin_rpc: plugin_rpc.clone(),
        desc: plugin_desc.clone(),
    };
    let lapce = lapce_exports(&store, &plugin_env);
    let instance = wasmer::Instance::new(&module, &lapce.chain_back(wasi))?;
    let mut plugin = NewPlugin {
        id,
        instance,
        env: plugin_env.clone(),
    };

    let handle_rpc = plugin
        .instance
        .exports
        .get_function("handle_rpc")
        .unwrap()
        .clone();
    let wasi_env = plugin_env.wasi_env.clone();
    thread::spawn(move || {
        for msg in io_rx {
            wasi_write_string(&wasi_env, &msg);
            let _ = handle_rpc.call(&[]);
        }
    });

    let local_rpc = rpc.clone();
    thread::spawn(move || {
        local_rpc.mainloop(&mut plugin);
    });

    plugin_rpc.plugin_server_loaded(rpc.clone());

    // thread::spawn(move || {
    //     let initialize =
    //         plugin.instance.exports.get_function("initialize").unwrap();
    //     wasi_write_object(
    //         &plugin.env.wasi_env,
    //         &PluginInfo {
    //             os: std::env::consts::OS.to_string(),
    //             arch: std::env::consts::ARCH.to_string(),
    //             configuration: plugin_desc.configuration,
    //         },
    //     );
    //     initialize.call(&[]).unwrap();
    // });

    // let (plugin_sender, plugin_receiver) =
    //     crossbeam_channel::unbounded::<PluginRpcMessage>();
    // let (proxy_sender, proxy_receiver) = crossbeam_channel::unbounded();
    // let mut rpc_handler = ProxyRpcHandler::new(plugin_sender);
    // rpc_handler.mainloop(proxy_receiver, &mut plugin);

    // for msg in plugin_receiver {
    //     wasi_write_object(&plugin_env.wasi_env, &msg.to_value().unwrap());
    // }

    Ok(())
}

pub fn find_all_plugins() -> Vec<PathBuf> {
    let mut plugin_paths = Vec::new();
    let home = home_dir().unwrap();
    let path = home.join(".lapce").join("plugins");
    let _ = path.read_dir().map(|dir| {
        dir.flat_map(|item| item.map(|p| p.path()).ok())
            .map(|dir| dir.join("plugin.toml"))
            .filter(|f| f.exists())
            .for_each(|f| plugin_paths.push(f))
    });
    plugin_paths
}

pub(crate) fn lapce_exports(
    store: &Store,
    plugin_env: &NewPluginEnv,
) -> ImportObject {
    macro_rules! lapce_export {
        ($($host_function:ident),+ $(,)?) => {
            wasmer::imports! {
                "lapce" => {
                    $(stringify!($host_function) =>
                        wasmer::Function::new_native_with_env(store, plugin_env.clone(), $host_function),)+
                }
            }
        }
    }

    lapce_export! {
        host_handle_notification,
    }
}

fn host_handle_notification(plugin_env: &NewPluginEnv) {
    let msg = wasi_read_string(&plugin_env.wasi_env).unwrap();
    if let Ok(rpc) = handle_plugin_server_message(&msg) {
        plugin_env.rpc.handle_rpc(rpc);
    }
}

fn wasi_write_string(wasi_env: &WasiEnv, buf: &str) {
    let mut state = wasi_env.state();
    let wasi_file = state.fs.stdin_mut().unwrap().as_mut().unwrap();
    writeln!(wasi_file, "{}\r", buf).unwrap();
}

fn wasi_read_string(wasi_env: &WasiEnv) -> Result<String> {
    let mut state = wasi_env.state();
    let wasi_file = state
        .fs
        .stdout_mut()?
        .as_mut()
        .ok_or_else(|| anyhow!("can't get stdout"))?;
    let mut buf = String::new();
    wasi_file.read_to_string(&mut buf)?;
    Ok(buf)
}
