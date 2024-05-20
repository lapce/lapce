use std::{path::Path, process::Command};

use anyhow::Result;
use tracing::{event, Level};

use crate::{
    proxy::{new_command, remote::Remote},
    workspace::ssh::Host,
};

pub struct SshRemote {
    pub host: Host,
}

impl SshRemote {
    #[cfg(windows)]
    const SSH_ARGS: &'static [&'static str] = &[];

    #[cfg(unix)]
    const SSH_ARGS: &'static [&'static str] = &[
        "-o",
        "ControlMaster=auto",
        "-o",
        "ControlPath=~/.ssh/cm_%C",
        "-o",
        "ControlPersist=30m",
        "-o",
        "ConnectTimeout=15",
    ];
}

impl Remote for SshRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut cmd = new_command("scp");

        cmd.args(Self::SSH_ARGS);

        if let Some(port) = self.host.port {
            cmd.arg("-P").arg(port.to_string());
        }

        let output = cmd
            .arg(local.as_ref())
            .arg(dbg!(format!("{}:{remote}", self.host.user_host())))
            .output()?;

        event!(Level::DEBUG, "{}", String::from_utf8_lossy(&output.stderr));
        event!(Level::DEBUG, "{}", String::from_utf8_lossy(&output.stdout));

        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("ssh");
        cmd.args(Self::SSH_ARGS);

        if let Some(port) = self.host.port {
            cmd.arg("-p").arg(port.to_string());
        }

        cmd.arg(self.host.user_host());

        if !std::env::var("LAPCE_DEBUG").unwrap_or_default().is_empty() {
            cmd.arg("-v");
        }

        cmd
    }
}
