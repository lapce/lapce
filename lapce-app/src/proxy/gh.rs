use std::{path::Path, process::Command};

use anyhow::Result;
use tracing::{trace, TraceLevel};

use crate::{
    proxy::{new_command, remote::Remote},
    workspace::gh::Host,
};

pub struct GhRemote {
    pub host: Host,
}

impl Remote for GhRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut cmd = new_command("gh");

        cmd.arg("cs");
        cmd.arg("-c");
        cmd.arg(self.host.codespace());

        cmd.arg("cp");
        cmd.arg("-e");
        cmd.arg(local.as_ref());
        cmd.arg(format!("remote:{remote}"));

        let output = cmd.output()?;

        trace!(
            TraceLevel::DEBUG,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        trace!(
            TraceLevel::DEBUG,
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );

        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("gh");

        cmd.arg("cs");

        cmd.arg("-c");
        cmd.arg(self.host.codespace());

        cmd.arg("ssh");

        if !std::env::var("LAPCE_DEBUG").unwrap_or_default().is_empty() {
            cmd.arg("-d");
        }

        cmd.arg("--");

        cmd
    }
}
