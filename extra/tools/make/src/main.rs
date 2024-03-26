mod notarize;

use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    path::PathBuf,
    process::{ExitCode, ExitStatus},
};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    subcommand: CliSubcommand,
}

#[derive(Subcommand)]
enum CliSubcommand {
    Staple {
        file: PathBuf,
    },
    Notarize {
        file: PathBuf,
        asc_provider: String,
        username: String,
        password: String,
    },
}

fn main() {
    let cli = Cli::parse();

    let result: Result<(), anyhow::Error> = match cli.subcommand {
        CliSubcommand::Staple { file } => {
            let file = file.to_str();
            match command_to_output(
                "xcrun",
                &["stapler", "staple", "--quiet", file.unwrap()],
                None,
            ) {
                Ok(v) => {
                    match v.code() {
                        Some(64) => {
                            Err(anyhow::anyhow!("Options appear malformed or are missing."))
                        }
                        Some(65) => {
                            Err(anyhow::anyhow!("The ticket data is invalid."))
                        }
                        Some(66) => {
                            Err(anyhow::anyhow!("The path cannot be found, is not code-signed, or is not of a supported file format, or, if the validate option is passed, the existing ticket is missing or invalid."))
                        }
                        Some(68) => {
                            Err(anyhow::anyhow!("The path has not been previously notarized or the ticketing service returns an unexpected response."))
                        }
                        Some(73) => {
                            Err(anyhow::anyhow!("The ticket has been retrieved from the ticketing service and was properly validated but the ticket could not be written out to disk."))
                        }
                        Some(77) => {
                            Err(anyhow::anyhow!("The ticket has been revoked by the ticketing service."))
                        }
                        Some(_) | None => {
                            Err(anyhow::anyhow!("Failed to staple build: unknown"))
                        }
                    }
                },
                Err(error) => {
                    Err(anyhow::anyhow!("Failed to staple the build: {error}"))
                }
            }
        }
        CliSubcommand::Notarize {
            file,
            asc_provider,
            username,
            password,
        } => notarize::notarize(file, asc_provider, username, password),
    };

    if let Err(error) = result {
        eprintln!("Command failed: {error}");
        std::process::exit(1);
    }
}

struct Output {
    stdout: String,
    stderr: String,
}

fn command_to_output<'a>(
    command: &'static str,
    arguments: &'a [&'a str],
    environment: Option<HashMap<String, String>>,
) -> Result<std::process::ExitStatus> {
    use std::process::Command;
    let mut cmd = Command::new(command);
    let cmd = cmd.args(arguments).envs(environment.unwrap_or_default());

    Ok(cmd.status()?)
}

fn command_to_string<'a>(
    command: &'static str,
    arguments: &'a [&'a str],
    environment: Option<HashMap<String, String>>,
) -> Result<Output> {
    use std::process::Command;
    let mut cmd = Command::new(command);
    let cmd = cmd.args(arguments).envs(environment.unwrap_or_default());

    let output = cmd.output()?;

    Ok(Output {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

