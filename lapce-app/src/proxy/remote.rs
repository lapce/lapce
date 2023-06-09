use std::{
    io::{BufReader, Write},
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{anyhow, Result};
use flate2::read::GzDecoder;
use lapce_core::{directory::Directory, meta};
use lapce_rpc::{
    core::CoreRpcHandler,
    proxy::{ProxyRpc, ProxyRpcHandler},
    stdio_transport, RpcMessage,
};
use thiserror::Error;

const UNIX_PROXY_SCRIPT: &[u8] = include_bytes!("../../../extra/proxy.sh");
const WINDOWS_PROXY_SCRIPT: &[u8] = include_bytes!("../../../extra/proxy.ps1");

#[derive(Clone, Copy, Error, Debug, PartialEq, Eq, strum_macros::Display)]
#[strum(ascii_case_insensitive)]
enum HostPlatform {
    UnknownOS,
    #[strum(serialize = "windows")]
    Windows,
    #[strum(serialize = "linux")]
    Linux,
    #[strum(serialize = "darwin")]
    Darwin,
    #[strum(serialize = "bsd")]
    Bsd,
}

/// serialise via strum to arch name that is used
/// in CI artefacts
#[derive(Clone, Copy, Error, Debug, PartialEq, Eq, strum_macros::Display)]
#[strum(ascii_case_insensitive)]
enum HostArchitecture {
    UnknownArch,
    #[strum(serialize = "x86_64")]
    AMD64,
    #[strum(serialize = "x86")]
    X86,
    #[strum(serialize = "aarch64")]
    ARM64,
    #[strum(serialize = "armv7")]
    ARM32v7,
    #[strum(serialize = "armhf")]
    ARM32v6,
}

pub trait Remote: Sized {
    fn home_dir(&self) -> Result<String> {
        let cmd = self
            .command_builder()
            .arg("echo")
            .arg("-n")
            .arg("$HOME")
            .stdout(Stdio::piped())
            .output()?;

        Ok(String::from_utf8(cmd.stdout)?)
    }

    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()>;

    fn command_builder(&self) -> Command;
}

pub fn start_remote(
    remote: impl Remote,
    core_rpc: CoreRpcHandler,
    proxy_rpc: ProxyRpcHandler,
) -> Result<()> {
    let proxy_version = meta::TAG;

    // start ssh CM connection in case where it doesn't handle
    // executing command properly on remote host
    // also print ssh debug output when used with LAPCE_DEBUG env
    match remote.command_builder().arg("lapce-no-command").output() {
        Ok(cmd) => {
            log::debug!(target: "lapce_app::proxy::start_remote::first_try", "{}", String::from_utf8_lossy(&cmd.stderr));
            log::debug!(target: "lapce_app:proxy::start_remote::first_try", "{}", String::from_utf8_lossy(&cmd.stdout));
        }
        Err(err) => {
            log::error!(target: "lapce_app::proxy::start_remote::first_try", "{err}");
            return Err(anyhow!(err));
        }
    }

    // Note about platforms:
    // Windows can use either cmd.exe, powershell.exe or pwsh.exe as
    // SSH shell, syntax logic varies significantly that's why we bet on
    // cmd.exe as it doesn't add unwanted newlines and use powershell only
    // for proxy install
    //
    // Unix-like systems due to POSIX, always have /bin/sh which should not
    // be necessary to use explicitly most of the time, as many wide-spread
    // shells retain similar syntax, although shells like Nushell might not
    // work (hopefully no one uses it as login shell)
    use HostPlatform::*;
    let (platform, architecture) = host_specification(&remote).unwrap();

    if platform == UnknownOS || architecture == HostArchitecture::UnknownArch {
        log::error!(target: "lapce_app::proxy::start_remote", "detected remote host: {platform}/{architecture}");
        return Err(anyhow!("Unknown OS and/or architecture"));
    }

    // ! Below paths have to be synced with what is
    // ! returned by Config::proxy_directory()
    let remote_proxy_path = match platform {
        Windows => format!(
            "%HOMEDRIVE%%HOMEPATH%\\AppData\\Local\\lapce\\{}\\data\\proxy",
            meta::NAME
        ),
        Darwin => format!(
            "~/Library/Application\\ Support/dev.lapce.{}/proxy",
            meta::NAME
        ),
        _ => {
            format!("~/.local/share/{}/proxy", meta::NAME.to_lowercase())
        }
    };

    let script_install = match platform {
        Windows => {
            let local_proxy_script =
                Directory::proxy_directory().unwrap().join("proxy.ps1");

            let mut proxy_script = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(&local_proxy_script)?;
            proxy_script.write_all(WINDOWS_PROXY_SCRIPT)?;

            let remote_proxy_script = "${env:TEMP}\\lapce-proxy.ps1";
            remote.upload_file(local_proxy_script, remote_proxy_script)?;

            let cmd = remote
                .command_builder()
                .args([
                    "powershell",
                    "-c",
                    remote_proxy_script,
                    "-version",
                    proxy_version,
                    "-directory",
                    &remote_proxy_path,
                ])
                .output()?;
            log::debug!(target: "lapce_app::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stderr));
            log::debug!(target: "lapce_app::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stdout));

            cmd.status
        }
        _ => {
            let local_proxy_script =
                Directory::proxy_directory().unwrap().join("proxy.sh");

            let mut proxy_script = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .open(&local_proxy_script)?;
            proxy_script.write_all(UNIX_PROXY_SCRIPT)?;

            let remote_proxy_script = "/tmp/lapce-proxy.sh";
            remote.upload_file(local_proxy_script, remote_proxy_script)?;

            let cmd = remote
                .command_builder()
                .args(["chmod", "+x", remote_proxy_script])
                .output()?;
            log::debug!(target: "lapce_app::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stderr));
            log::debug!(target: "lapce_app::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stdout));

            let cmd = remote
                .command_builder()
                .args([remote_proxy_script, proxy_version, &remote_proxy_path])
                .output()?;
            log::debug!(target: "lapce_app::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stderr));
            log::debug!(target: "lapce_app::proxy::upload_file", "{}", String::from_utf8_lossy(&cmd.stdout));

            cmd.status
        }
    };

    let remote_proxy_file = match platform {
        Windows => format!("{remote_proxy_path}\\lapce.exe"),
        _ => format!("{remote_proxy_path}/lapce"),
    };

    let proxy_filename = format!("lapce-proxy-{platform}-{architecture}");

    log::debug!(target: "lapce_app::proxy::start_remote", "remote proxy path: {remote_proxy_path}");

    if !script_install.success() {
        let cmd = match platform {
            Windows => remote
                .command_builder()
                .args(["dir", &remote_proxy_file])
                .status()?,
            _ => remote
                .command_builder()
                .arg("test")
                .arg("-e")
                .arg(&remote_proxy_file)
                .status()?,
        };
        if !cmd.success() {
            let local_proxy_file = Directory::proxy_directory()
                .ok_or_else(|| anyhow!("can't find proxy directory"))?
                .join(&proxy_filename);
            // remove possibly outdated proxy
            if local_proxy_file.exists() {
                // TODO: add proper proxy version detection and update proxy
                // when needed
                std::fs::remove_file(&local_proxy_file)?;
            }
            let url = format!("https://github.com/lapce/lapce/releases/download/{proxy_version}/{proxy_filename}.gz");
            log::debug!(target: "lapce_app::proxy::start_remote", "proxy download URI: {url}");
            let mut resp = reqwest::blocking::get(url).expect("request failed");
            if resp.status().is_success() {
                let mut out = std::fs::File::create(&local_proxy_file)
                    .expect("failed to create file");
                let mut gz = GzDecoder::new(&mut resp);
                std::io::copy(&mut gz, &mut out).expect("failed to copy content");
            } else {
                log::error!(target: "lapce_app::proxy::start_remote", "proxy download failed with: {}", resp.status());
            }

            match platform {
                // Windows creates all dirs in provided path
                Windows => remote
                    .command_builder()
                    .arg("mkdir")
                    .arg(remote_proxy_path)
                    .status()?,
                // Unix needs -p to do same
                _ => remote
                    .command_builder()
                    .arg("mkdir")
                    .arg("-p")
                    .arg(remote_proxy_path)
                    .status()?,
            };

            remote.upload_file(&local_proxy_file, &remote_proxy_file)?;
            if platform != Windows {
                remote
                    .command_builder()
                    .arg("chmod")
                    .arg("+x")
                    .arg(&remote_proxy_file)
                    .status()?;
            }
        }
    }

    let mut child = match platform {
        // Force cmd.exe usage to resolve %envvar% variables
        Windows => remote
            .command_builder()
            .args(["cmd", "/c"])
            .arg(&remote_proxy_file)
            .arg("--proxy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?,
        _ => remote
            .command_builder()
            .arg(&remote_proxy_file)
            .arg("--proxy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?,
    };
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("can't find stdin"))?;
    let stdout = BufReader::new(
        child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("can't find stdout"))?,
    );
    log::debug!(target: "lapce_app::proxy::start_remote", "process id: {}", child.id());

    let (writer_tx, writer_rx) = crossbeam_channel::unbounded();
    let (reader_tx, reader_rx) = crossbeam_channel::unbounded();
    stdio_transport(stdin, writer_rx, stdout, reader_tx);

    let local_proxy_rpc = proxy_rpc.clone();
    let local_writer_tx = writer_tx.clone();
    std::thread::spawn(move || {
        for msg in local_proxy_rpc.rx() {
            match msg {
                ProxyRpc::Request(id, rpc) => {
                    let _ = local_writer_tx.send(RpcMessage::Request(id, rpc));
                }
                ProxyRpc::Notification(rpc) => {
                    let _ = local_writer_tx.send(RpcMessage::Notification(rpc));
                }
                ProxyRpc::Shutdown => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
            }
        }
    });

    std::thread::spawn(move || {
        for msg in reader_rx {
            match msg {
                RpcMessage::Request(id, req) => {
                    let writer_tx = writer_tx.clone();
                    let core_rpc = core_rpc.clone();
                    std::thread::spawn(move || match core_rpc.request(req) {
                        Ok(resp) => {
                            let _ = writer_tx.send(RpcMessage::Response(id, resp));
                        }
                        Err(e) => {
                            let _ = writer_tx.send(RpcMessage::Error(id, e));
                        }
                    });
                }
                RpcMessage::Notification(n) => {
                    core_rpc.notification(n);
                }
                RpcMessage::Response(id, resp) => {
                    proxy_rpc.handle_response(id, Ok(resp));
                }
                RpcMessage::Error(id, err) => {
                    proxy_rpc.handle_response(id, Err(err));
                }
            }
        }
    });

    Ok(())
}

fn host_specification(
    remote: &impl Remote,
) -> Result<(HostPlatform, HostArchitecture)> {
    use HostArchitecture::*;
    use HostPlatform::*;

    let cmd = remote.command_builder().args(["uname", "-sm"]).output();

    let spec = match cmd {
        Ok(cmd) => {
            let stdout = String::from_utf8_lossy(&cmd.stdout).to_lowercase();
            let stdout = stdout.trim();
            log::debug!(target: "lapce_app::proxy::host_specification", "{}", &stdout);
            match stdout {
                // If empty, then we probably deal with Windows and not Unix
                // or something went wrong with command output
                "" => {
                    // Try cmd explicitly
                    let cmd = remote
                        .command_builder()
                        .args(["cmd", "/c", "echo %OS% %PROCESSOR_ARCHITECTURE%"])
                        .output();
                    match cmd {
                        Ok(cmd) => {
                            let stdout =
                                String::from_utf8_lossy(&cmd.stdout).to_lowercase();
                            let stdout = stdout.trim();
                            log::debug!(target: "lapce_app::proxy::host_specification", "{}", &stdout);
                            match stdout.split_once(' ') {
                                Some((os, arch)) => (parse_os(os), parse_arch(arch)),
                                None => {
                                    // PowerShell fallback
                                    let cmd = remote
                                            .command_builder()
                                            .args(["echo", "\"${env:OS} ${env:PROCESSOR_ARCHITECTURE}\""])
                                            .output();
                                    match cmd {
                                        Ok(cmd) => {
                                            let stdout =
                                                String::from_utf8_lossy(&cmd.stdout)
                                                    .to_lowercase();
                                            let stdout = stdout.trim();
                                            log::debug!(target: "lapce_app::proxy::host_specification", "{}", &stdout);
                                            match stdout.split_once(' ') {
                                                Some((os, arch)) => {
                                                    (parse_os(os), parse_arch(arch))
                                                }
                                                None => (UnknownOS, UnknownArch),
                                            }
                                        }
                                        Err(e) => {
                                            log::error!(target: "lapce_app::proxy::host_specification", "{e}");
                                            (UnknownOS, UnknownArch)
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!(target: "lapce_app::proxy::host_specification", "{e}");
                            (UnknownOS, UnknownArch)
                        }
                    }
                }
                v => {
                    if let Some((os, arch)) = v.split_once(' ') {
                        (parse_os(os), parse_arch(arch))
                    } else {
                        (UnknownOS, UnknownArch)
                    }
                }
            }
        }
        Err(e) => {
            log::error!(target: "lapce_app::proxy::host_specification", "{e}");
            (UnknownOS, UnknownArch)
        }
    };
    Ok(spec)
}

fn parse_arch(arch: &str) -> HostArchitecture {
    use HostArchitecture::*;
    // processor architectures be like that
    match arch {
        "amd64" | "x64" | "x86_64" => AMD64,
        "x86" | "i386" | "i586" | "i686" => X86,
        "arm" | "armhf" | "armv6" => ARM32v6,
        "armv7" | "armv7l" => ARM32v7,
        "arm64" | "armv8" | "aarch64" => ARM64,
        _ => UnknownArch,
    }
}

fn parse_os(os: &str) -> HostPlatform {
    use HostPlatform::*;
    match os {
        "linux" => Linux,
        "darwin" => Darwin,
        "windows_nt" => Windows,
        v if v.ends_with("bsd") => Bsd,
        _ => UnknownOS,
    }
}
