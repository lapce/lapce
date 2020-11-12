use anyhow::{anyhow, Result};
use openssh;
use parking_lot::Mutex;
use std::fs;
use std::io::prelude::*;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::str::FromStr;
use std::sync::Arc;
use tokio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct SshSession {
    pub session: openssh::Session,
    rt: Arc<tokio::runtime::Runtime>,
}

pub struct SshPathEntry {
    pub path: PathBuf,
    pub is_dir: bool,
}

impl SshSession {
    pub fn new(user: &str, host: &str) -> Result<SshSession> {
        let rt = tokio::runtime::Runtime::new()?;
        let session = rt.block_on(async {
            openssh::Session::connect(
                &format!("{}@{}", user, host),
                openssh::KnownHosts::Accept,
            )
            .await
        })?;
        Ok(SshSession {
            session,
            rt: Arc::new(rt),
        })
    }

    pub fn command(&self, cmd: &str) -> Result<String> {
        self.rt.block_on(async {
            let stdout = self.session.shell(cmd).output().await?.stdout;
            Ok(String::from_utf8(stdout)?)
        })
    }

    pub fn get_pwd(&self) -> Result<PathBuf> {
        let s = self.command("pwd")?;
        let pwd = PathBuf::from(&s.split("\n").collect::<Vec<&str>>()[0]);
        Ok(pwd)
    }

    pub fn write_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let mut sftp = self.session.sftp();
        self.rt.block_on(async {
            let mut w = sftp.write_to(path).await?;
            w.write_all(bytes).await?;
            w.close().await?;
            Ok(())
        })
    }

    pub fn append_file(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let mut sftp = self.session.sftp();
        self.rt.block_on(async {
            let mut w = sftp.append_to(path).await?;
            w.write_all(bytes).await?;
            w.close().await?;
            Ok(())
        })
    }

    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let mut sftp = self.session.sftp();
        self.rt.block_on(async {
            let mut r = sftp.read_from(path).await?;
            let mut buffer = Vec::new();
            r.read_to_end(&mut buffer).await?;
            r.close().await?;
            Ok(buffer)
        })
    }

    pub fn read_dirs(&self, path: &PathBuf) -> Result<Vec<PathBuf>> {
        let s = self.command(&format!("ls -p {}", path.to_str().unwrap()))?;
        let mut paths = Vec::new();
        for part in s.split("\n") {
            let part = part.trim();
            if part.ends_with("/") {
                paths.push(path.join(part));
            }
        }
        Ok(paths)
    }

    pub fn read_dir(&self, path: &str) -> Result<Vec<String>> {
        let s = self.command(&format!("cd {} && git ls-files", path))?;
        let base_path = PathBuf::from(path);
        let mut paths = Vec::new();
        for part in s.split("\n") {
            let part = part.trim();
            if part == "" {
                continue;
            }
            if let Some(path) = base_path.join(part).to_str() {
                paths.push(path.to_string());
            }
        }
        Ok(paths)
    }

    pub fn close(mut self) -> Result<()> {
        let rt = self.rt.clone();
        rt.block_on(async move {
            self.session.close().await?;
            Ok(())
        })
    }
}
