use std::{path::Path, process::Command};

use anyhow::Result;
use tracing::{event, Level};

use crate::{
    proxy::{new_command, remote::Remote},
    workspace::ts::Host,
};

pub struct TsRemote {
    pub host: Host,
}

#[rustfmt::skip]
impl TsRemote {
    const TS_PROG: &'static str = "tailscale";
    const TS_ARGS: &'static [&'static str] = &["ssh"];
}

impl Remote for TsRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut cmd = new_command(TsRemote::TS_PROG);
        cmd.args(TsRemote::TS_ARGS);

        cmd.arg(self.host.user_host());

        cmd.arg("cp");
        cmd.arg("-e");
        cmd.arg(local.as_ref());
        cmd.arg(format!("remote:{remote}"));

        let output = cmd.output()?;

        event!(Level::DEBUG, "{}", String::from_utf8_lossy(&output.stderr));
        event!(Level::DEBUG, "{}", String::from_utf8_lossy(&output.stdout));

        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("gh");

        cmd.arg("cs");

        cmd.arg("-c");
        cmd.arg(self.host.user_host());

        cmd.arg("ssh");

        if !std::env::var("LAPCE_DEBUG").unwrap_or_default().is_empty() {
            cmd.arg("-d");
        }

        cmd.arg("--");

        cmd
    }
}
