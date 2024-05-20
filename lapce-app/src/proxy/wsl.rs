use std::{path::Path, process::Command};

use anyhow::Result;

use crate::{
    proxy::{new_command, remote::Remote},
    workspace::wsl::Host,
};

pub struct WslRemote {
    pub host: Host,
}

impl Remote for WslRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut wsl_path = Path::new(r"\\wsl.localhost\").join(&self.host.host);
        if !wsl_path.exists() {
            wsl_path = Path::new(r"\\wsl$").join(&self.host.host);
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
        cmd.arg("-d").arg(&self.host.host).arg("--");
        cmd
    }
}
