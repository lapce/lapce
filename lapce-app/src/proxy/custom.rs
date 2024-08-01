use std::{path::Path, process::Command};

use anyhow::Result;
use tracing::{trace, TraceLevel};

use crate::{
    proxy::{new_command, remote::Remote},
    workspace::custom::Host,
};

pub struct CustomRemote {
    pub host: Host,
    pub output: Option<String>,
}

impl CustomRemote {
    fn start_command(&self) -> Result<Option<String>> {
        if let Some(start_args) = &self.host.start_args {
            let mut cmd = new_command(&self.host.program);

            for arg in start_args {
                cmd.arg(arg);
            }

            trace!(TraceLevel::DEBUG, "{:?}", cmd.get_args());

            let output = cmd.output()?;

            trace!(
                TraceLevel::DEBUG,
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            trace!(TraceLevel::DEBUG, "{}", stdout);
            return Ok(Some(stdout.trim().to_string()));
        }

        Ok(None)
    }
}

impl Remote for CustomRemote {
    fn upload_file(&self, local: impl AsRef<Path>, remote: &str) -> Result<()> {
        let mut cmd = new_command(dbg!(&self.host.program));

        for arg in &self.host.copy_args {
            cmd.arg(dbg!(arg
                .replace("{local}", &local.as_ref().display().to_string())
                .replace("{remote}", remote)
                .replace(
                    "{output}",
                    self.output.as_deref().unwrap_or_default()
                )));
        }

        _ = dbg!(cmd.get_args());

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
        let mut cmd = new_command(dbg!(&self.host.program));

        for arg in &self.host.exec_args {
            cmd.arg(
                arg.replace("{output}", self.output.as_deref().unwrap_or_default()),
            );
        }

        _ = dbg!(cmd.get_args());

        cmd
    }

    // fn stop_command(&self) -> Result<()> {
    //     if let Some(start_args) = &self.custom.stop_args {
    //         let mut cmd = new_command(&self.custom.program);

    //         for arg in start_args {
    //             cmd.arg(arg.replace(
    //                 "{output}",
    //                 self.output.as_deref().unwrap_or_default(),
    //             ));
    //         }

    //         log::debug!(target: "lapce_data::proxy::stop_command", "{:?}", cmd.get_args());

    //         let output = cmd.output()?;

    //         log::debug!(target: "lapce_data::proxy::stop_command", "{}", String::from_utf8_lossy(&output.stderr));
    //         log::debug!(target: "lapce_data::proxy::stop_command", "{}", String::from_utf8_lossy(&output.stdout));
    //     }

    //     Ok(())
    // }
}
