use std::{path::Path, process::Command};

use anyhow::Result;

use crate::workspace::WslHost;

use super::{new_command, remote::Remote};

pub struct WslRemote {
    pub wsl: WslHost,
}

impl Remote for WslRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut wsl_path = Path::new(r"\\wsl.localhost\").join(&self.wsl.host);
        if !wsl_path.exists() {
            wsl_path = Path::new(r"\\wsl$").join(&self.wsl.host);
        }
        wsl_path = if remote.starts_with('~') {
            let home_dir = self.home_dir()?;
            wsl_path.join(remote.replacen('~', &home_dir, 1))
        } else {
            wsl_path.join(remote)
        };
        std::fs::copy(local, wsl_path)?;
        Ok(())
    }

    fn command_builder(&self) -> Command {
        let mut cmd = new_command("wsl");
        cmd.arg("-d").arg(&self.wsl.host).arg("--");
        cmd
    }
}
