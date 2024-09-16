use std::rc::Rc;

use lapce_rpc::dap_types::{ConfigSource, RunDebugConfig, RunDebugProgram};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{command::InternalCommand, debug::RunDebugMode, window_tab::CommonData};

#[derive(Serialize, Deserialize)]
struct CargoArgs {
    #[serde(rename = "cargoArgs")]
    pub cargo_args: Vec<String>,

    #[serde(rename = "cargoExtraArgs")]
    pub cargo_extra_args: Vec<String>,

    #[serde(rename = "executableArgs")]
    pub executable_args: Vec<String>,
}

#[derive(Serialize, Deserialize)]
struct RustArgs {
    pub args: CargoArgs,
    pub kind: String,
    pub label: String,
    pub location: lsp_types::LocationLink,
}

#[derive(Clone)]
pub struct CodeLensData {
    common: Rc<CommonData>,
}

impl CodeLensData {
    pub fn new(common: Rc<CommonData>) -> Self {
        Self { common }
    }

    pub fn run(&self, command: &str, args: Vec<Value>) {
        match command {
            "rust-analyzer.runSingle" | "rust-analyzer.debugSingle" => {
                let mode = if command == "rust-analyzer.runSingle" {
                    RunDebugMode::Run
                } else {
                    RunDebugMode::Debug
                };
                if let Some(config) = self.get_rust_command_config(&args, mode) {
                    self.common
                        .internal_command
                        .send(InternalCommand::RunAndDebug { mode, config });
                }
            }
            _ => {
                tracing::debug!("todo {:}", command);
            }
        }
    }

    fn get_rust_command_config(
        &self,
        args: &[Value],
        mode: RunDebugMode,
    ) -> Option<RunDebugConfig> {
        if let Some(args) = args.first() {
            let Ok(mut cargo_args) =
                serde_json::from_value::<RustArgs>(args.clone())
            else {
                tracing::error!("serde error");
                return None;
            };
            cargo_args
                .args
                .cargo_args
                .extend(cargo_args.args.cargo_extra_args);

            let mut prelaunch = None;
            let mut program = cargo_args.kind;
            let mut tracing_output = false;
            let mut ty = None;
            if mode == RunDebugMode::Debug
                && cargo_args
                    .args
                    .cargo_args
                    .first()
                    .map(|x| x == "run")
                    .unwrap_or_default()
                && &program == "cargo"
            {
                ty = Some("lldb".to_owned());
                cargo_args.args.cargo_args[0] = "build".to_owned();
                let mut args = Vec::with_capacity(cargo_args.args.cargo_args.len());
                std::mem::swap(&mut args, &mut cargo_args.args.cargo_args);
                args.push("--message-format=json".to_owned());
                prelaunch = Some(RunDebugProgram {
                    program: "cargo".to_string(),
                    args: Some(args),
                });
                cargo_args
                    .args
                    .cargo_args
                    .extend(cargo_args.args.executable_args);
                program = "____".to_owned();
                tracing_output = true;
            } else if !cargo_args.args.executable_args.is_empty() {
                cargo_args.args.cargo_args.push("--".to_string());
                cargo_args
                    .args
                    .cargo_args
                    .extend(cargo_args.args.executable_args);
            }
            Some(RunDebugConfig {
                ty,
                name: cargo_args.label,
                program,
                args: Some(cargo_args.args.cargo_args),
                cwd: None,
                env: None,
                prelaunch,
                debug_command: None,
                dap_id: Default::default(),
                tracing_output,
                config_source: ConfigSource::RustCodeLens,
            })
        } else {
            tracing::error!("no args");
            None
        }
    }
}
